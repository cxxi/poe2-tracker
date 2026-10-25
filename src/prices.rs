//! Prix poe.ninja (PoE2), en cache local pour répondre instantanément.
//!
//! Les valeurs poe.ninja sont en divines ; on les convertit en exalts au taux du moment.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::art::USER_AGENT;

const API: &str = "https://poe.ninja/poe2/api/economy";
const EXCHANGE_TYPES: &[&str] = &[
    "Currency", "Fragments", "Runes", "Essences", "SoulCores", "Idols", "Expedition", "Ritual",
    "Delirium", "Breach", "Abyss", "UncutGems", "LineageSupportGems",
];
const UNIQUE_TYPES: &[&str] = &[
    "UniqueWeapons", "UniqueArmours", "UniqueAccessories", "UniqueFlasks", "UniqueJewels",
    "UniqueCharms", "UniqueSanctumRelics",
];
const MAX_AGE: Duration = Duration::from_secs(30 * 60);
/// En dessous, le prix d'un unique n'est pas fiable.
pub const MIN_LISTINGS: u32 = 5;

#[derive(Default, Serialize, Deserialize)]
pub struct PriceData {
    pub league: String,
    pub fetched: u64,
    pub exalted_per_divine: f64,
    pub currency: HashMap<String, f64>,
    /// Même valeur indexée par identifiant court ("exalted", "divine", "alch"…),
    /// celui qu'utilisent les annonces de l'API de trade.
    #[serde(default)]
    pub currency_ids: HashMap<String, f64>,
    /// "nom|base" -> (valeur en divines, nombre d'annonces)
    pub uniques: HashMap<String, (f64, u32)>,
}

impl PriceData {
    pub fn currency(&self, name: &str) -> Option<f64> {
        self.currency.get(name).copied()
    }

    pub fn unique(&self, name: &str, base: &str) -> Option<(f64, u32)> {
        self.uniques.get(&format!("{name}|{base}")).copied()
    }

    /// Valeur en exalts d'un prix d'annonce ("3 divine", "40 exalted"…).
    pub fn listing_to_ex(&self, amount: f64, currency_id: &str) -> Option<f64> {
        let unit = match currency_id {
            "divine" => 1.0,
            id => *self.currency_ids.get(id)?,
        };
        Some(self.to_ex(amount * unit))
    }

    pub fn to_ex(&self, divines: f64) -> f64 {
        divines * self.exalted_per_divine
    }

    pub fn fmt(&self, divines: f64) -> String {
        if divines >= 1.0 {
            format!("{divines:.1} div")
        } else {
            format!("{:.1} ex", self.to_ex(divines))
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn get(path: &str, league: &str, kind: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let url = format!("{API}/{path}?league={}&type={kind}", league.replace(' ', "+"));
    Ok(ureq::get(&url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call()?.into_json()?)
}

fn download(league: &str) -> Result<PriceData, Box<dyn std::error::Error>> {
    let mut data = PriceData { league: league.to_string(), fetched: now(), ..Default::default() };
    for t in EXCHANGE_TYPES {
        let j = get("exchange/current/overview", league, t)?;
        if data.exalted_per_divine == 0.0 {
            data.exalted_per_divine = j["core"]["rates"]["exalted"].as_f64().unwrap_or(0.0);
        }
        let names: HashMap<&str, &str> = j["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| Some((i["id"].as_str()?, i["name"].as_str()?)))
            .collect();
        for line in j["lines"].as_array().into_iter().flatten() {
            if let (Some(id), Some(v)) = (line["id"].as_str(), line["primaryValue"].as_f64()) {
                if let Some(name) = names.get(id) {
                    data.currency.insert(name.to_string(), v);
                }
                data.currency_ids.insert(id.to_string(), v);
            }
        }
    }
    for t in UNIQUE_TYPES {
        let j = get("stash/current/item/overview", league, t)?;
        for line in j["lines"].as_array().into_iter().flatten() {
            let (Some(name), Some(base), Some(v)) = (line["name"].as_str(), line["baseType"].as_str(), line["primaryValue"].as_f64()) else {
                continue;
            };
            let listings = line["listingCount"].as_u64().unwrap_or(0) as u32;
            let key = format!("{name}|{base}");
            // Plusieurs variantes du même unique : on garde la plus échangée.
            if data.uniques.get(&key).is_none_or(|(_, n)| listings > *n) {
                data.uniques.insert(key, (v, listings));
            }
        }
    }
    Ok(data)
}

/// Prix partagés avec l'interface, rafraîchis en arrière-plan toutes les 30 minutes.
#[derive(Clone)]
pub struct Prices {
    pub data: Arc<Mutex<Option<PriceData>>>,
}

impl Prices {
    pub fn start(cache: PathBuf, league: String) -> Prices {
        let path = cache.join("prices.json");
        let cached: Option<PriceData> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
        let cached = cached.filter(|d| d.league == league);
        let prices = Prices { data: Arc::new(Mutex::new(cached)) };
        let shared = prices.data.clone();
        std::thread::spawn(move || loop {
            // Plus personne ne lit ces prix (changement de ligue) : le fil s'arrête.
            if Arc::strong_count(&shared) == 1 {
                return;
            }
            let stale = shared.lock().unwrap().as_ref().is_none_or(|d| now().saturating_sub(d.fetched) > MAX_AGE.as_secs());
            if stale {
                match download(&league) {
                    Ok(d) => {
                        let _ = std::fs::create_dir_all(&cache);
                        let _ = std::fs::write(&path, serde_json::to_string(&d).unwrap_or_default());
                        *shared.lock().unwrap() = Some(d);
                    }
                    Err(e) => eprintln!("prix poe.ninja indisponibles : {e}"),
                }
            }
            std::thread::sleep(Duration::from_secs(60));
        });
        prices
    }
}
