//! Prix poe.ninja (PoE2), en cache local pour répondre instantanément.
//!
//! Les valeurs poe.ninja sont en divines ; on les convertit en exalts au taux du moment.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// poe.ninja recalcule ses taux environ toutes les heures : 15 minutes suffisent.
const MAX_AGE: Duration = Duration::from_secs(15 * 60);
/// Préfixe des images de poe.ninja.
pub const IMAGE_HOST: &str = "https://web.poecdn.com";

/// Une ligne du marché d'échange (onglet Marché).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketLine {
    pub id: String,
    pub name: String,
    /// Catégorie poe.ninja (« Currency », « Runes »…).
    pub category: String,
    /// Chemin de l'icône sur `IMAGE_HOST`.
    pub image: String,
    /// Valeur en divines.
    pub value: f64,
    /// Volume échangé (en divines).
    pub volume: f64,
    /// Variation sur 7 jours, en %.
    pub change: f64,
    /// Variation jour par jour sur 7 jours, en %.
    pub spark: Vec<f64>,
    /// Identifiant de sa page sur poe.ninja (« divine-orb »).
    #[serde(default)]
    pub details_id: String,
}

/// « UniqueArmours » -> « unique-armours » : le type tel qu'il s'écrit dans les adresses de
/// poe.ninja.
fn kebab(kind: &str) -> String {
    let mut out = String::new();
    for (i, c) in kind.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// Ligue telle qu'elle s'écrit dans les adresses de poe.ninja : « Forbidden Rites » ->
/// « forbiddenrites », « HC Forbidden Rites » -> « forbiddenriteshc ».
fn league_slug(league: &str) -> String {
    let (name, hc) = match league.strip_prefix("HC ") {
        Some(rest) => (rest, "hc"),
        None => (league, ""),
    };
    let slug: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    format!("{slug}{hc}")
}
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
    /// Marché d'échange, toutes catégories.
    #[serde(default)]
    pub market: Vec<MarketLine>,
    /// "nom|base" -> (type, identifiant) de la page de l'unique sur poe.ninja.
    #[serde(default)]
    pub unique_pages: HashMap<String, (String, String)>,
}

impl PriceData {
    pub fn currency(&self, name: &str) -> Option<f64> {
        self.currency.get(name).copied()
    }

    pub fn unique(&self, name: &str, base: &str) -> Option<(f64, u32)> {
        self.uniques.get(&format!("{name}|{base}")).copied()
    }

    /// Unique par son seul nom : type de base, valeur en divines et nombre d'annonces de sa
    /// variante la plus échangée.
    pub fn unique_by_name(&self, name: &str) -> Option<(&str, f64, u32)> {
        let prefix = format!("{name}|");
        self.uniques
            .iter()
            .filter_map(|(k, (v, n))| Some((k.strip_prefix(&prefix)?, *v, *n)))
            .max_by_key(|(_, _, n)| *n)
    }

    /// Valeur en exalts d'un prix d'annonce ("3 divine", "40 exalted"…).
    pub fn listing_to_ex(&self, amount: f64, currency_id: &str) -> Option<f64> {
        let unit = match currency_id {
            "divine" => 1.0,
            id => *self.currency_ids.get(id)?,
        };
        Some(self.to_ex(amount * unit))
    }

    /// Page poe.ninja d'un unique (`name` et `base`) ou d'un objet du marché d'échange (`base`).
    pub fn page_url(&self, unique: bool, name: &str, base: &str) -> Option<String> {
        let league = league_slug(&self.league);
        if unique {
            let (kind, id) = self.unique_pages.get(&format!("{name}|{base}"))?;
            return Some(format!("https://poe.ninja/poe2/economy/{league}/{kind}/{id}"));
        }
        let line = self.market.iter().find(|l| l.name == base && !l.details_id.is_empty())?;
        Some(format!("https://poe.ninja/poe2/economy/{league}/{}/{}", kebab(&line.category), line.details_id))
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
        let items: HashMap<&str, (&str, &str, &str)> = j["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| Some((i["id"].as_str()?, (i["name"].as_str()?, i["image"].as_str().unwrap_or(""), i["detailsId"].as_str().unwrap_or("")))))
            .collect();
        for line in j["lines"].as_array().into_iter().flatten() {
            if let (Some(id), Some(v)) = (line["id"].as_str(), line["primaryValue"].as_f64()) {
                if let Some((name, image, details)) = items.get(id) {
                    data.currency.insert(name.to_string(), v);
                    data.market.push(MarketLine {
                        id: id.to_string(),
                        name: name.to_string(),
                        category: t.to_string(),
                        image: image.to_string(),
                        value: v,
                        volume: line["volumePrimaryValue"].as_f64().unwrap_or(0.0),
                        change: line["sparkline"]["totalChange"].as_f64().unwrap_or(0.0),
                        spark: line["sparkline"]["data"].as_array().into_iter().flatten().filter_map(|x| x.as_f64()).collect(),
                        details_id: details.to_string(),
                    });
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
                if let Some(id) = line["detailsId"].as_str() {
                    data.unique_pages.insert(key.clone(), (kebab(t), id.to_string()));
                }
                data.uniques.insert(key, (v, listings));
            }
        }
    }
    Ok(data)
}

/// Famille d'un objet du marché et son rang dans la famille : « Greater Jeweller's Orb » ->
/// (« Jeweller's Orb », 2). Les rangs vont de Lesser (0) à Perfect (3) ; les orbes de sacrifice
/// (« Xoph's Orb of Sacrifice ») vont ensemble.
pub fn family(name: &str) -> (String, u8) {
    let (rank, rest) = [("Lesser ", 0), ("Greater ", 2), ("Perfect ", 3)]
        .iter()
        .find_map(|(p, r)| name.strip_prefix(p).map(|rest| (*r, rest)))
        .unwrap_or((1, name));
    if let Some((owner, tail)) = rest.split_once("'s ") {
        if !owner.contains(' ') && tail.starts_with("Orb of ") {
            return (tail.to_string(), rank);
        }
    }
    (rest.to_string(), rank)
}

/// Ordre logique des monnaies, comme on les range : les plus précieuses, puis l'artisanat de
/// base dans l'ordre d'usage, les orbes spéciales et enfin le reste. Par familles (`family`).
const CURRENCY_ORDER: &[&str] = &[
    "Mirror of Kalandra",
    "Divine Orb",
    "Chaos Orb",
    "Exalted Orb",
    "Regal Orb",
    "Orb of Alchemy",
    "Orb of Augmentation",
    "Orb of Transmutation",
    "Orb of Chance",
    "Chance Shard",
    "Orb of Annulment",
    "Vaal Orb",
    "Fracturing Orb",
    "Jeweller's Orb",
    "Artificer's Orb",
    "Artificer's Shard",
    "Gemcutter's Prism",
    "Glassblower's Bauble",
    "Blacksmith's Whetstone",
    "Armourer's Scrap",
    "Arcanist's Etcher",
    "Scroll of Wisdom",
    "Orb of Extraction",
    "Hinekora's Lock",
    "Architect's Orb",
    "Crystallised Corruption",
    "Vaal Cultivation Orb",
    "Core Destabiliser",
    "Vaal Siphoner",
    "Ancient Infuser",
    "Vaal Catalysing Infuser",
    "Vaal Arcanist's Infuser",
    "Vaal Armourer's Infuser",
    "Vaal Blacksmith's Infuser",
    "Orb of Sacrifice",
    "Cryptic Key",
];

/// Trie les lignes par famille. Monnaies dans leur ordre logique ; le reste (et les monnaies
/// inconnues, après) par famille de la plus chère à la moins chère. Dans une famille, du rang
/// le plus bas au plus haut (Lesser, normal, Greater, Perfect).
pub fn sort_by_family(lines: &mut [MarketLine]) {
    let mut best: HashMap<String, f64> = HashMap::new();
    for l in lines.iter() {
        let v = best.entry(family(&l.name).0).or_insert(0.0);
        *v = v.max(l.value);
    }
    let order = |f: &str| {
        CURRENCY_ORDER
            .iter()
            .position(|c| *c == f)
            .unwrap_or(usize::MAX)
    };
    lines.sort_by(|a, b| {
        let (fa, ra) = family(&a.name);
        let (fb, rb) = family(&b.name);
        order(&fa)
            .cmp(&order(&fb))
            .then(best[&fb].total_cmp(&best[&fa]))
            .then(fa.cmp(&fb))
            .then(ra.cmp(&rb))
    });
}

/// Historique du cours d'un objet du marché d'échange (page de détail de poe.ninja) : un point
/// par jour, du plus ancien au plus récent, en exalts. Le cours en divines est ramené aux exalts
/// au taux du jour ; la divine elle-même a son cours en exalts.
pub fn history(league: &str, category: &str, details_id: &str, exalted_per_divine: f64) -> Result<Vec<(String, f64)>, Box<dyn std::error::Error>> {
    let url = format!("{API}/exchange/current/details?league={}&type={category}&id={details_id}", league.replace(' ', "+"));
    let j: serde_json::Value = ureq::get(&url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call()?.into_json()?;
    let pairs = j["pairs"].as_array().cloned().unwrap_or_default();
    let pair = |id: &str| pairs.iter().find(|p| p["id"] == id);
    let (pair, per) = match (pair("divine"), pair("exalted")) {
        (Some(p), _) => (p, exalted_per_divine),
        (None, Some(p)) => (p, 1.0),
        _ => return Ok(Vec::new()),
    };
    let mut points: Vec<(String, f64)> = pair["history"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| Some((h["timestamp"].as_str()?.get(..10)?.to_string(), h["rate"].as_f64()? * per)))
        .collect();
    points.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(points)
}

/// Prix partagés avec l'interface, rafraîchis en arrière-plan toutes les 15 minutes, ou sur
/// demande.
#[derive(Clone)]
pub struct Prices {
    pub data: Arc<Mutex<Option<PriceData>>>,
    /// Rafraîchissement demandé (bouton « Actualiser »).
    wanted: Arc<AtomicBool>,
    /// Téléchargement en cours.
    busy: Arc<AtomicBool>,
}

impl Prices {
    pub fn start(cache: PathBuf, league: String) -> Prices {
        // Un fichier par ligue : le marché peut en regarder une autre que celle du personnage.
        let slug: String = league.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        let path = cache.join(format!("prices-{slug}.json"));
        let cached: Option<PriceData> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
        let cached = cached.filter(|d| d.league == league);
        let prices = Prices { data: Arc::new(Mutex::new(cached)), wanted: Arc::default(), busy: Arc::default() };
        let (shared, wanted, busy) = (prices.data.clone(), prices.wanted.clone(), prices.busy.clone());
        std::thread::spawn(move || loop {
            // Plus personne ne lit ces prix (changement de ligue) : le fil s'arrête.
            if Arc::strong_count(&shared) == 1 {
                return;
            }
            // Cache d'une version sans le marché ou sans les pages poe.ninja : on le refait aussi.
            let stale = shared.lock().unwrap().as_ref().is_none_or(|d| now().saturating_sub(d.fetched) > MAX_AGE.as_secs() || d.market.is_empty() || d.unique_pages.is_empty());
            if stale || wanted.swap(false, Ordering::Relaxed) {
                busy.store(true, Ordering::Relaxed);
                match download(&league) {
                    Ok(d) => {
                        let _ = std::fs::create_dir_all(&cache);
                        let _ = std::fs::write(&path, serde_json::to_string(&d).unwrap_or_default());
                        *shared.lock().unwrap() = Some(d);
                    }
                    Err(e) => eprintln!("prix poe.ninja indisponibles : {e}"),
                }
                busy.store(false, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_secs(2));
        });
        prices
    }

    /// Demande un rafraîchissement immédiat.
    pub fn refresh(&self) {
        self.wanted.store(true, Ordering::Relaxed);
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed) || self.wanted.load(Ordering::Relaxed)
    }

    /// Âge des prix, en secondes.
    pub fn age(&self) -> Option<u64> {
        self.data
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| now().saturating_sub(d.fetched))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ninja_addresses() {
        assert_eq!(kebab("UniqueArmours"), "unique-armours");
        assert_eq!(kebab("LineageSupportGems"), "lineage-support-gems");
        assert_eq!(league_slug("Forbidden Rites"), "forbiddenrites");
        assert_eq!(league_slug("HC Forbidden Rites"), "forbiddenriteshc");
    }

    #[test]
    fn families() {
        assert_eq!(
            family("Greater Jeweller's Orb"),
            ("Jeweller's Orb".into(), 2)
        );
        assert_eq!(
            family("Orb of Augmentation"),
            ("Orb of Augmentation".into(), 1)
        );
        assert_eq!(
            family("Perfect Orb of Augmentation"),
            ("Orb of Augmentation".into(), 3)
        );
        assert_eq!(family("Xoph's Orb of Sacrifice").0, "Orb of Sacrifice");
        assert_eq!(
            family("Lesser Essence of Ice"),
            ("Essence of Ice".into(), 0)
        );
        let line = |n: &str, v: f64| MarketLine {
            name: n.into(),
            value: v,
            ..Default::default()
        };
        let mut l = vec![
            line("Lesser Jeweller's Orb", 0.01),
            line("Orb of Transmutation", 0.001),
            line("Unknown Thing", 9.0),
            line("Exalted Orb", 0.0015),
            line("Divine Orb", 1.0),
            line("Perfect Jeweller's Orb", 0.5),
            line("Greater Exalted Orb", 0.02),
        ];
        sort_by_family(&mut l);
        let names: Vec<&str> = l.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Divine Orb",
                "Exalted Orb",
                "Greater Exalted Orb",
                "Orb of Transmutation",
                "Lesser Jeweller's Orb",
                "Perfect Jeweller's Orb",
                "Unknown Thing"
            ]
        );
    }
}
