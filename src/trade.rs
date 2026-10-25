//! Estimation du prix des rares via l'API de trade officielle (sans connexion).
//!
//! On cherche des objets comparables en vente : même catégorie, vie et résistances totales
//! (pseudo-stats), autres mods explicites avec ~15 % de tolérance. Si trop peu de résultats,
//! seconde recherche plus souple (tous les mods sauf un). Les limites de débit annoncées par
//! le serveur sont respectées avec une marge.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use regex::Regex;
use serde_json::{Value, json};

use crate::art::USER_AGENT;
use crate::item::{Item, ModKind, Rarity};
use crate::prices::Prices;

const API: &str = "https://www.pathofexile.com/api/trade2";
const SITE: &str = "https://www.pathofexile.com/trade2/search/poe2";
/// Tolérance sur les valeurs des mods recherchés.
const TOLERANCE: f64 = 0.85;
/// En dessous, on refait une recherche plus souple.
const ENOUGH: u64 = 5;
const FETCH_COUNT: usize = 10;

pub fn category(item_class: &str) -> Option<&'static str> {
    Some(match item_class {
        "Helmets" => "armour.helmet",
        "Body Armours" => "armour.chest",
        "Gloves" => "armour.gloves",
        "Boots" => "armour.boots",
        "Shields" => "armour.shield",
        "Foci" => "armour.focus",
        "Bucklers" => "armour.buckler",
        "Quivers" => "armour.quiver",
        "Amulets" => "accessory.amulet",
        "Rings" => "accessory.ring",
        "Belts" => "accessory.belt",
        "Staves" => "weapon.staff",
        "Wands" => "weapon.wand",
        "Sceptres" => "weapon.sceptre",
        "Quarterstaves" => "weapon.warstaff",
        "Bows" => "weapon.bow",
        "Crossbows" => "weapon.crossbow",
        "Claws" => "weapon.claw",
        "Daggers" => "weapon.dagger",
        "One Hand Swords" => "weapon.onesword",
        "One Hand Axes" => "weapon.oneaxe",
        "One Hand Maces" => "weapon.onemace",
        "Two Hand Swords" => "weapon.twosword",
        "Two Hand Axes" => "weapon.twoaxe",
        "Two Hand Maces" => "weapon.twomace",
        "Spears" => "weapon.spear",
        "Flails" => "weapon.flail",
        "Talismans" => "weapon.talisman",
        "Jewels" => "jewel",
        "Charms" => "flask.charm",
        "Life Flasks" => "flask.life",
        "Mana Flasks" => "flask.mana",
        _ => return None,
    })
}

/// Un objet vaut-il une estimation au marché ?
pub fn priceable(it: &Item) -> bool {
    matches!(it.rarity, Rarity::Rare | Rarity::Magic) && !it.unidentified && category(&it.item_class).is_some()
}

static TAG_PAIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]|]+)\|([^\]]+)\]").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]").unwrap());
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+?(\d+(?:\.\d+)?)").unwrap());

/// "+21 to maximum Life" -> ("# to maximum Life", 21). Plusieurs nombres -> moyenne.
pub fn template(line: &str) -> (String, Option<f64>) {
    let line = TAG_PAIR.replace_all(line, "$2");
    let line = TAG.replace_all(&line, "$1");
    let nums: Vec<f64> = NUMBER.captures_iter(&line).filter_map(|c| c[1].parse().ok()).collect();
    let value = (!nums.is_empty()).then(|| nums.iter().sum::<f64>() / nums.len() as f64);
    (NUMBER.replace_all(&line, "#").into_owned(), value)
}

/// Modèle de stat du trade -> identifiant, par type (explicit, implicit…).
pub struct StatIndex {
    ids: HashMap<(String, String), String>,
}

impl StatIndex {
    fn download() -> Result<Value, Box<dyn std::error::Error>> {
        Ok(ureq::get(&format!("{API}/data/stats")).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(30)).call()?.into_json()?)
    }

    pub fn load(cache: &Path) -> StatIndex {
        let path = cache.join("trade_stats.json");
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < Duration::from_secs(86400))
            .unwrap_or(false);
        let cached = || std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let data = if fresh { cached() } else { None }.or_else(|| match StatIndex::download() {
            Ok(v) => {
                let _ = std::fs::create_dir_all(cache);
                let _ = std::fs::write(&path, v.to_string());
                Some(v)
            }
            Err(e) => {
                eprintln!("stats du trade indisponibles : {e}");
                cached()
            }
        });
        let mut ids = HashMap::new();
        for group in data.iter().flat_map(|d| d["result"].as_array()).flatten() {
            let kind = group["id"].as_str().unwrap_or_default().to_string();
            for e in group["entries"].as_array().into_iter().flatten() {
                if let (Some(id), Some(text)) = (e["id"].as_str(), e["text"].as_str()) {
                    ids.entry((kind.clone(), text.to_string())).or_insert_with(|| id.to_string());
                }
            }
        }
        StatIndex { ids }
    }

    fn get(&self, kind: &str, template: &str) -> Option<&String> {
        self.ids.get(&(kind.to_string(), template.to_string()))
    }
}

/// Un critère de recherche : identifiant, valeur minimale, description lisible, importance.
#[derive(Clone, Debug)]
struct Criterion {
    id: String,
    min: Option<f64>,
    label: String,
    weight: f64,
}

static RES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(\d+)% to (Fire|Cold|Lightning) Resistance$").unwrap());
static ALL_RES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(\d+)% to all Elemental Resistances$").unwrap());
static LIFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(\d+) to maximum Life$").unwrap());

fn criteria(it: &Item, index: &StatIndex) -> Vec<Criterion> {
    let mut out = Vec::new();
    let (mut life, mut res) = (0.0, 0.0);
    for (m, kind) in it.mods.iter().zip(&it.mod_kinds) {
        if *kind != ModKind::Explicit {
            continue;
        }
        // Vie et résistances élémentaires : regroupées en pseudo-stats.
        if let Some(c) = LIFE.captures(m) {
            life += c[1].parse::<f64>().unwrap_or(0.0);
            continue;
        }
        if let Some(c) = RES.captures(m) {
            res += c[1].parse::<f64>().unwrap_or(0.0);
            continue;
        }
        if let Some(c) = ALL_RES.captures(m) {
            res += 3.0 * c[1].parse::<f64>().unwrap_or(0.0);
            continue;
        }
        let (tpl, value) = template(m);
        let Some(id) = index.get("explicit", &tpl) else { continue };
        // Les mods de « remplissage » pèsent peu : ce sont eux qu'on lâche en premier.
        let weight = if tpl.contains("Level of all") {
            10.0
        } else if tpl.contains("Movement Speed") || tpl.contains("Spirit") || tpl.contains("Spell Damage") {
            5.0
        } else if tpl.contains("Accuracy") || tpl.contains("Light Radius") || tpl.contains("Stun Threshold") || tpl.contains("Thorns") {
            0.5
        } else {
            2.0
        };
        out.push(Criterion { id: id.clone(), min: value.map(|v| (v * TOLERANCE).floor()), label: m.clone(), weight });
    }
    if life > 0.0 {
        out.push(Criterion { id: "pseudo.pseudo_total_life".into(), min: Some((life * TOLERANCE).floor()), label: format!("+{life} vie au total"), weight: 3.0 });
    }
    if res > 0.0 {
        out.push(Criterion {
            id: "pseudo.pseudo_total_elemental_resistance".into(),
            min: Some((res * TOLERANCE).floor()),
            label: format!("+{res}% résistances élémentaires au total"),
            weight: 3.0,
        });
    }
    out.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    out
}

/// Au-delà de 1,5 fois la puissance de base, une annonce n'est plus comparable.
const POWER_MAX: f64 = 1.5;

/// Puissance de base de l'objet, toujours exigée (même en recherche élargie) : entre 85 % et
/// 150 % des DPS de l'arme, ou de son armure / évasion / bouclier d'énergie. Sans ça, une
/// arme faible se compare à des armes de fin de jeu qui partagent un mod anodin.
fn equipment_filters(it: &Item) -> serde_json::Map<String, Value> {
    let mut f = serde_json::Map::new();
    let get = |k: &str| it.props.get(k).copied().unwrap_or(0.0);
    let range = |v: f64| json!({"min": (v * TOLERANCE).floor(), "max": (v * POWER_MAX).ceil()});
    let dps = (get("phys_avg") + get("ele_avg")) * get("aps");
    if dps > 0.0 {
        f.insert("dps".into(), range(dps));
    }
    for (prop, id) in [("Armour", "ar"), ("Evasion Rating", "ev"), ("Energy Shield", "es")] {
        let v = get(prop);
        if v > 0.0 {
            f.insert(id.into(), range(v));
        }
    }
    f
}

fn query(it: &Item, crit: &[Criterion], relaxed: bool) -> Value {
    let filters: Vec<Value> = crit
        .iter()
        .map(|c| match c.min {
            Some(min) => json!({"id": c.id, "value": {"min": min}}),
            None => json!({"id": c.id}),
        })
        .collect();
    let group = if relaxed {
        json!({"type": "count", "value": {"min": crit.len().saturating_sub(1).max(1)}, "filters": filters})
    } else {
        json!({"type": "and", "filters": filters})
    };
    let mut type_filters = json!({"category": {"option": category(&it.item_class)}, "rarity": {"option": "nonunique"}});
    if it.rarity == Rarity::Magic {
        type_filters["rarity"] = json!({"option": "magic"});
    }
    let mut filters = json!({"type_filters": {"filters": type_filters}});
    let equipment = equipment_filters(it);
    if !equipment.is_empty() {
        filters["equipment_filters"] = json!({"filters": equipment});
    }
    json!({
        "query": {"status": {"option": "online"}, "stats": [group], "filters": filters},
        "sort": {"price": "asc"}
    })
}

/// Respect des limites annoncées par le serveur ("5:10:60,15:60:300" = 5 requêtes / 10 s…).
struct Limiter {
    rules: Vec<(usize, u64)>,
    sent: VecDeque<Instant>,
    blocked_until: Option<Instant>,
}

impl Limiter {
    fn new(rules: &[(usize, u64)]) -> Limiter {
        Limiter { rules: rules.to_vec(), sent: VecDeque::new(), blocked_until: None }
    }

    /// Temps à attendre avant la prochaine requête (marge d'une requête par règle).
    fn wait(&self) -> Duration {
        let now = Instant::now();
        let mut wait = self.blocked_until.map(|t| t.saturating_duration_since(now)).unwrap_or_default();
        for (max, period) in &self.rules {
            let window = Duration::from_secs(*period);
            let recent: Vec<&Instant> = self.sent.iter().filter(|t| now.duration_since(**t) < window).collect();
            if recent.len() + 1 >= *max {
                if let Some(oldest) = recent.first() {
                    wait = wait.max((**oldest + window).saturating_duration_since(now));
                }
            }
        }
        wait
    }

    fn record(&mut self, headers: Option<&str>) {
        self.sent.push_back(Instant::now());
        while self.sent.len() > 2000 {
            self.sent.pop_front();
        }
        if let Some(h) = headers {
            let rules: Vec<(usize, u64)> = h
                .split(',')
                .filter_map(|r| {
                    let mut p = r.split(':');
                    Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?))
                })
                .collect();
            if !rules.is_empty() {
                self.rules = rules;
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum Estimate {
    Pending(String),
    Done {
        total: u64,
        /// Prix des annonces les moins chères, en exalts.
        prices_ex: Vec<f64>,
        relaxed: bool,
        url: String,
        criteria: Vec<String>,
    },
    Failed(String),
}

/// Nombre d'annonces les moins chères retenues pour la valeur : au-delà, les annonces qui
/// correspondent aussi aux critères valent cher pour d'autres raisons (meilleure base, mods
/// en plus), pas pour ce que l'objet possède.
const VALUE_FROM: usize = 5;

impl Estimate {
    /// Valeur estimée en exalts : prix médian des 5 annonces comparables les moins chères.
    pub fn value_ex(&self) -> Option<f64> {
        let Estimate::Done { prices_ex, .. } = self else { return None };
        let mut p = prices_ex.clone();
        p.sort_by(f64::total_cmp);
        p.truncate(VALUE_FROM);
        p.get(p.len() / 2).copied()
    }
}

type Shared = Arc<Mutex<HashMap<String, Estimate>>>;

pub struct Trade {
    tx: Sender<Item>,
    pub results: Shared,
}

impl Trade {
    pub fn start(ctx: egui::Context, cache: PathBuf, league: String, prices: Prices) -> Trade {
        let (tx, rx) = channel::<Item>();
        let results: Shared = Arc::new(Mutex::new(HashMap::new()));
        let shared = results.clone();
        std::thread::spawn(move || worker(rx, shared, ctx, cache, league, prices));
        Trade { tx, results }
    }

    pub fn request(&self, it: &Item) {
        let mut r = self.results.lock().unwrap();
        if matches!(r.get(&it.raw), Some(Estimate::Done { .. })) {
            return; // déjà estimé
        }
        r.insert(it.raw.clone(), Estimate::Pending("en file d'attente".into()));
        let _ = self.tx.send(it.clone());
    }

    pub fn get(&self, it: &Item) -> Option<Estimate> {
        self.results.lock().unwrap().get(&it.raw).cloned()
    }
}

fn set(shared: &Shared, ctx: &egui::Context, key: &str, e: Estimate) {
    shared.lock().unwrap().insert(key.to_string(), e);
    ctx.request_repaint();
}

fn worker(rx: Receiver<Item>, shared: Shared, ctx: egui::Context, cache: PathBuf, league: String, prices: Prices) {
    let index = StatIndex::load(&cache);
    let mut search_limit = Limiter::new(&[(5, 10), (15, 60), (30, 300)]);
    let mut fetch_limit = Limiter::new(&[(12, 4), (16, 12), (50, 300)]);
    let league_path = league.replace(' ', "%20");
    let mut queue: VecDeque<Item> = VecDeque::new();
    loop {
        // On traite d'abord les objets les plus récents ; au-delà de 5 en attente, on lâche les plus vieux.
        match rx.recv() {
            Ok(it) => queue.push_back(it),
            Err(_) => return,
        }
        while let Ok(it) = rx.try_recv() {
            queue.push_back(it);
        }
        while queue.len() > 5 {
            if let Some(old) = queue.pop_front() {
                set(&shared, &ctx, &old.raw, Estimate::Failed("ignoré : trop d'objets copiés d'affilée".into()));
            }
        }
        while let Some(it) = queue.pop_back() {
            let result = estimate(&it, &index, &mut search_limit, &mut fetch_limit, &league, &league_path, &prices, |msg| {
                set(&shared, &ctx, &it.raw, Estimate::Pending(msg));
            });
            set(&shared, &ctx, &it.raw, result.unwrap_or_else(Estimate::Failed));
            while let Ok(more) = rx.try_recv() {
                queue.push_back(more);
            }
        }
    }
}

fn throttle(limit: &Limiter, report: &dyn Fn(String)) {
    let wait = limit.wait();
    if !wait.is_zero() {
        report(format!("limite de l'API de trade : attente {} s", wait.as_secs() + 1));
        std::thread::sleep(wait);
    }
}

/// Envoie une requête ; en cas de 429, attend le délai demandé et réessaie une fois.
fn send(limit: &mut Limiter, report: &dyn Fn(String), req: impl Fn() -> Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    for _ in 0..2 {
        throttle(limit, report);
        match req() {
            Ok(resp) => {
                limit.record(resp.header("x-rate-limit-ip"));
                return resp.into_json().map_err(|e| e.to_string());
            }
            Err(ureq::Error::Status(429, resp)) => {
                let secs: u64 = resp.header("retry-after").and_then(|v| v.parse().ok()).unwrap_or(60);
                limit.record(resp.header("x-rate-limit-ip"));
                limit.blocked_until = Some(Instant::now() + Duration::from_secs(secs));
                report(format!("limite de l'API de trade dépassée : attente {secs} s"));
            }
            Err(e) => return Err(format!("API de trade : {e}")),
        }
    }
    Err("API de trade : limite toujours dépassée".into())
}

#[allow(clippy::too_many_arguments)]
fn estimate(
    it: &Item,
    index: &StatIndex,
    search_limit: &mut Limiter,
    fetch_limit: &mut Limiter,
    league: &str,
    league_path: &str,
    prices: &Prices,
    report: impl Fn(String),
) -> Result<Estimate, String> {
    let crit = criteria(it, index);
    if crit.is_empty() && equipment_filters(it).is_empty() {
        return Err("aucun mod reconnu pour chercher des objets comparables".into());
    }
    let search_url = format!("{API}/search/poe2/{league_path}");
    let search = |relaxed: bool, limit: &mut Limiter| {
        report(if relaxed { "recherche élargie…".into() } else { "recherche d'objets comparables…".into() });
        let body = query(it, &crit, relaxed);
        send(limit, &report, || ureq::post(&search_url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).send_json(body.clone()))
    };
    let mut relaxed = false;
    let mut res = search(false, search_limit)?;
    if res["total"].as_u64().unwrap_or(0) < ENOUGH && crit.len() >= 3 {
        relaxed = true;
        res = search(true, search_limit)?;
    }
    let total = res["total"].as_u64().unwrap_or(0);
    let id = res["id"].as_str().unwrap_or_default().to_string();
    let url = format!("{SITE}/{}/{id}", league.replace(' ', "%20"));
    let hashes: Vec<&str> = res["result"].as_array().into_iter().flatten().filter_map(Value::as_str).take(FETCH_COUNT).collect();
    let mut prices_ex = Vec::new();
    if !hashes.is_empty() {
        report("lecture des annonces…".into());
        let fetch_url = format!("{API}/fetch/{}?query={id}", hashes.join(","));
        let listings = send(fetch_limit, &report, || ureq::get(&fetch_url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call())?;
        let data = prices.data.lock().unwrap();
        for r in listings["result"].as_array().into_iter().flatten() {
            let price = &r["listing"]["price"];
            if let (Some(amount), Some(cur), Some(d)) = (price["amount"].as_f64(), price["currency"].as_str(), data.as_ref()) {
                if let Some(ex) = d.listing_to_ex(amount, cur) {
                    prices_ex.push(ex);
                }
            }
        }
    }
    Ok(Estimate::Done { total, prices_ex, relaxed, url, criteria: crit.iter().map(|c| c.label.clone()).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates() {
        assert_eq!(template("+21 to maximum Life"), ("# to maximum Life".into(), Some(21.0)));
        assert_eq!(template("Adds 1 to 6 Lightning damage to Attacks"), ("Adds # to # Lightning damage to Attacks".into(), Some(3.5)));
        assert_eq!(template("+10% to [Resistances|Cold Resistance]").0, "#% to Cold Resistance");
        assert_eq!(template("2.6 Life Regeneration per second"), ("# Life Regeneration per second".into(), Some(2.6)));
    }

    #[test]
    fn limiter_waits_when_window_is_full() {
        let mut l = Limiter::new(&[(3, 10)]);
        assert!(l.wait().is_zero());
        l.record(None);
        assert!(l.wait().is_zero());
        l.record(None);
        assert!(!l.wait().is_zero(), "marge d'une requête : la 3e doit attendre");
    }

    #[test]
    fn median() {
        let e = Estimate::Done { total: 3, prices_ex: vec![5.0, 1.0, 3.0], relaxed: false, url: String::new(), criteria: vec![] };
        assert_eq!(e.value_ex(), Some(3.0));
        // Cas réel (arbalète niveau 11, recherche élargie) : les annonces hors de prix ne comptent pas.
        let prices_ex = vec![1.0, 1.0, 2.0, 4.0, 17.0, 666.0, 999.0, 1348.0, 2697.0, 74847.0];
        let e = Estimate::Done { total: 10, prices_ex, relaxed: true, url: String::new(), criteria: vec![] };
        assert_eq!(e.value_ex(), Some(2.0));
    }
}

/// Estimation unique et bloquante (outil en ligne de commande `--price`).
pub fn estimate_once(it: &Item, cache: &Path, league: &str, prices: &Prices) -> Estimate {
    let index = StatIndex::load(cache);
    let mut search_limit = Limiter::new(&[(5, 10), (15, 60), (30, 300)]);
    let mut fetch_limit = Limiter::new(&[(12, 4), (16, 12), (50, 300)]);
    let league_path = league.replace(' ', "%20");
    estimate(it, &index, &mut search_limit, &mut fetch_limit, league, &league_path, prices, |m| eprintln!("… {m}"))
        .unwrap_or_else(Estimate::Failed)
}
