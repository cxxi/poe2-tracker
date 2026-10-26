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
    /// Valeur sur l'objet : on cherche entre 85 % et 150 % (au-delà, l'annonce est d'un autre
    /// niveau, surtout sans DPS ni défense pour fixer la puissance de l'objet).
    value: Option<f64>,
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
        out.push(Criterion { id: id.clone(), value, label: m.clone(), weight });
    }
    if life > 0.0 {
        out.push(Criterion { id: "pseudo.pseudo_total_life".into(), value: Some(life), label: format!("+{life} vie au total"), weight: 3.0 });
    }
    if res > 0.0 {
        out.push(Criterion {
            id: "pseudo.pseudo_total_elemental_resistance".into(),
            value: Some(res),
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

/// Arme sans DPS (bâton, baguette, sceptre) : elle se juge sur sa base et ses mods de sorts.
fn caster_weapon(it: &Item) -> bool {
    it.rarity == Rarity::Rare && category(&it.item_class).is_some_and(|c| c.starts_with("weapon.")) && equipment_filters(it).is_empty()
}

/// Mods explicites de l'objet (préfixes et suffixes), pour les rares et les magiques. Une
/// annonce comparable en a au moins autant : une place libre se crafte, elle vaut plus qu'un
/// mod faible.
fn filled_affixes(it: &Item) -> Option<usize> {
    if !matches!(it.rarity, Rarity::Rare | Rarity::Magic) {
        return None;
    }
    // Texte avancé (Ctrl+Alt+C) : un bloc par mod, même hybride (deux lignes, un seul mod).
    let blocks = it.raw.lines().filter(|l| l.starts_with("{ Prefix Modifier") || l.starts_with("{ Suffix Modifier")).count();
    Some(if blocks > 0 { blocks } else { it.mod_kinds.iter().filter(|k| **k == ModKind::Explicit).count() })
}

/// Tous les mods de l'objet, sauf les `dropped` plus faibles (les derniers, `crit` étant trié) ;
/// leur place reste exigée : les annonces retenues ont un autre mod à cet endroit, pas une
/// place libre.
/// `instant` : seulement les annonces en achat immédiat (prix réels, vendeurs présents).
/// Sans critère de mod (base seule) : même type de base, niveau d'objet au moins égal.
fn query(it: &Item, crit: &[Criterion], dropped: usize, instant: bool) -> Value {
    let kept = &crit[..crit.len().saturating_sub(dropped)];
    let filters: Vec<Value> = kept
        .iter()
        .map(|c| match c.value {
            Some(v) => json!({"id": c.id, "value": {"min": (v * TOLERANCE).floor(), "max": (v * POWER_MAX).ceil()}}),
            None => json!({"id": c.id}),
        })
        .collect();
    let mut stats = Vec::new();
    if !filters.is_empty() {
        stats.push(json!({"type": "and", "filters": filters}));
    }
    // « Au plus N places libres » s'écrit « au moins M mods » : un objet plein n'a pas la stat
    // des places libres, un filtre « max 0 » ne le trouverait pas.
    if let Some(filled) = filled_affixes(it).filter(|n| *n > 0) {
        stats.push(json!({"type": "and", "filters": [{"id": "pseudo.pseudo_number_of_affix_mods", "value": {"min": filled}}]}));
    }
    let mut type_filters = json!({"category": {"option": category(&it.item_class)}, "rarity": {"option": "nonunique"}});
    if it.rarity == Rarity::Magic {
        type_filters["rarity"] = json!({"option": "magic"});
    }
    let mut filters = json!({"type_filters": {"filters": type_filters}});
    let equipment = equipment_filters(it);
    if !equipment.is_empty() {
        filters["equipment_filters"] = json!({"filters": equipment});
    }
    let mut q = json!({"status": {"option": if instant { "securable" } else { "online" }}, "stats": stats, "filters": filters});
    // Arme de caster : pas de DPS pour situer sa puissance, et sa base décide de l'élément de
    // ses mods de sorts ; on reste sur le même type de base.
    if caster_weapon(it) {
        q["type"] = json!(it.base);
    }
    // Base à crafter : rien d'intéressant dans les mods, c'est la base qui se vend.
    if crit.is_empty() && it.rarity != Rarity::Magic {
        q["type"] = json!(it.base);
        q["filters"]["misc_filters"] = json!({"filters": {"ilvl": {"min": it.item_level}}});
        q["filters"]["type_filters"]["filters"]["rarity"] = json!({"option": "nonunique"});
    }
    json!({ "query": q, "sort": {"price": "asc"} })
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

    /// `headers` : règles annoncées (« x-rate-limit-ip ») ; `state` : requêtes déjà comptées
    /// par le serveur dans chaque fenêtre (« x-rate-limit-ip-state »), d'autres programmes ou
    /// une autre fenêtre ouverte compris. On attend une fenêtre entière avant d'y toucher la
    /// limite.
    fn record(&mut self, headers: Option<&str>, state: Option<&str>) {
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
        for r in state.into_iter().flat_map(|s| s.split(',')) {
            let n: Vec<u64> = r.split(':').filter_map(|p| p.parse().ok()).collect();
            let [hits, period, penalty] = n[..] else { continue };
            let max = self.rules.iter().find(|(_, p)| *p == period).map(|(m, _)| *m as u64);
            let until = if penalty > 0 {
                Some(Instant::now() + Duration::from_secs(penalty))
            } else if max.is_some_and(|m| hits + 1 >= m) {
                Some(Instant::now() + Duration::from_secs(period))
            } else {
                None
            };
            if let Some(u) = until {
                self.blocked_until = Some(self.blocked_until.map_or(u, |b| b.max(u)));
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Estimate {
    Pending(String),
    Done {
        total: u64,
        /// Prix des annonces les moins chères, en exalts.
        prices_ex: Vec<f64>,
        /// Mods les plus faibles lâchés pour trouver assez d'annonces (recherche élargie).
        relaxed: usize,
        url: String,
        criteria: Vec<String>,
        /// Annonces en achat immédiat (sinon : en ligne, dernier recours).
        instant: bool,
        /// Prix de la base seule (aucun mod intéressant).
        base_only: bool,
    },
    Failed(String),
}

/// Nombre d'annonces les moins chères retenues pour la valeur : au-delà, les annonces qui
/// correspondent aussi aux critères valent cher pour d'autres raisons (meilleure base, mods
/// en plus), pas pour ce que l'objet possède.
const VALUE_FROM: usize = 5;
/// En dessous de ce nombre d'annonces avec un prix, pas d'estimation : une annonce seule peut
/// être un prix fantaisiste (vu : 34 905 ex sur une recherche élargie).
pub const MIN_PRICED: usize = 3;
/// Au-delà de ce multiple (ou en deçà de cette fraction) de la médiane des annonces les moins
/// chères, une annonce est jugée aberrante.
const OUTLIER: f64 = 5.0;

impl Estimate {
    /// Prix retenus : les `VALUE_FROM` annonces les moins chères, sans les valeurs aberrantes
    /// (plus de 5 fois au-dessus ou en dessous de leur médiane : un prix fantaisiste, ou une
    /// annonce bradée qui ne dit rien du prix des autres).
    fn kept(&self) -> Vec<f64> {
        let Estimate::Done { prices_ex, .. } = self else { return vec![] };
        let mut p = prices_ex.clone();
        p.sort_by(f64::total_cmp);
        p.truncate(VALUE_FROM);
        let Some(mid) = p.get(p.len() / 2).copied() else { return p };
        p.retain(|v| *v <= mid.max(0.1) * OUTLIER && *v >= mid / OUTLIER);
        p
    }

    /// Valeur estimée en exalts : prix médian des annonces retenues, s'il y en a au moins
    /// `MIN_PRICED`.
    pub fn value_ex(&self) -> Option<f64> {
        let p = self.kept();
        if p.len() < MIN_PRICED {
            return None;
        }
        p.get(p.len() / 2).copied()
    }

    /// Fourchette des annonces retenues (la moins chère, la plus chère).
    pub fn range_ex(&self) -> Option<(f64, f64)> {
        let p = self.kept();
        (p.len() >= MIN_PRICED).then(|| (p[0], p[p.len() - 1]))
    }

    /// Confiance dans l'estimation, en pourcentage : 100 % avec 5 annonces retenues trouvées
    /// avec tous les mods de l'objet ; moins 10 points par annonce retenue en moins, moins
    /// 15 points par mod lâché.
    pub fn confidence_pct(&self) -> u32 {
        let Estimate::Done { relaxed, instant, .. } = self else { return 0 };
        let kept = self.kept().len().min(VALUE_FROM) as i32;
        if (kept as usize) < MIN_PRICED {
            return 0;
        }
        let mut pct = 100 - 10 * (VALUE_FROM as i32 - kept) - 15 * *relaxed as i32;
        if !instant {
            pct -= 20;
        }
        pct.clamp(10, 100) as u32
    }

    /// Confiance en mots : bonne (75 % et plus), moyenne (50 % et plus), faible.
    pub fn confidence(&self) -> &'static str {
        match self.confidence_pct() {
            75.. => "bonne",
            50.. => "moyenne",
            _ => "faible",
        }
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
        let saved = load_saved(&cache, &league).into_iter().map(|(raw, (_, e))| (raw, e)).collect();
        let results: Shared = Arc::new(Mutex::new(saved));
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
            if let Ok(e) = &result {
                save(&cache, &league, &it.raw, e);
            }
            set(&shared, &ctx, &it.raw, result.unwrap_or_else(Estimate::Failed));
            while let Ok(more) = rx.try_recv() {
                queue.push_back(more);
            }
        }
    }
}

/// Estimations gardées d'une session à l'autre : un jour au plus (les prix bougent), pour la
/// ligue en cours.
const SAVED: &str = "trade_estimates.json";
const SAVED_FOR: u64 = 24 * 3600;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Saved {
    league: String,
    /// Texte de l'objet -> (date en secondes Unix, estimation).
    items: HashMap<String, (u64, Estimate)>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load_saved(cache: &Path, league: &str) -> HashMap<String, (u64, Estimate)> {
    let saved: Saved = std::fs::read_to_string(cache.join(SAVED)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if saved.league != league {
        return HashMap::new();
    }
    let now = now_secs();
    saved.items.into_iter().filter(|(_, (t, e))| now.saturating_sub(*t) < SAVED_FOR && matches!(e, Estimate::Done { .. })).collect()
}

fn save(cache: &Path, league: &str, raw: &str, e: &Estimate) {
    let mut items = load_saved(cache, league);
    items.insert(raw.to_string(), (now_secs(), e.clone()));
    let saved = Saved { league: league.to_string(), items };
    let _ = std::fs::create_dir_all(cache);
    let _ = std::fs::write(cache.join(SAVED), serde_json::to_string(&saved).unwrap_or_default());
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
                limit.record(resp.header("x-rate-limit-ip"), resp.header("x-rate-limit-ip-state"));
                return resp.into_json().map_err(|e| e.to_string());
            }
            Err(ureq::Error::Status(429, resp)) => {
                let secs: u64 = resp.header("retry-after").and_then(|v| v.parse().ok()).unwrap_or(60);
                limit.record(resp.header("x-rate-limit-ip"), None);
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
    let base_only = crit.is_empty();
    if base_only && (it.rarity == Rarity::Magic || it.item_level == 0) && equipment_filters(it).is_empty() {
        return Err("aucun mod reconnu pour chercher des objets comparables".into());
    }
    let search_url = format!("{API}/search/poe2/{league_path}");
    let search = |dropped: usize, instant: bool, limit: &mut Limiter| {
        report(match (dropped, instant) {
            (0, true) => "recherche d'objets comparables (achat immédiat)…".into(),
            (n, true) => format!("recherche élargie, {n} mod(s) lâché(s) (achat immédiat)…"),
            (_, false) => "recherche parmi les annonces en ligne…".into(),
        });
        let body = query(it, &crit, dropped, instant);
        send(limit, &report, || ureq::post(&search_url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).send_json(body.clone()))
    };
    // Deux recherches au plus, pour ménager l'API : achat immédiat avec tous les mods ; s'il y a
    // trop peu de résultats, on lâche d'un coup la moitié la plus faible (leurs places restent
    // exigées). Les annonces seulement « en ligne » ne servent pas : prix souvent fantaisistes.
    let (mut relaxed, instant) = (0, true);
    let mut res = search(0, true, search_limit)?;
    if res["total"].as_u64().unwrap_or(0) < ENOUGH && crit.len() >= 2 {
        relaxed = crit.len() / 2;
        res = search(relaxed, true, search_limit)?;
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
    let mut criteria: Vec<String> = crit.iter().map(|c| c.label.clone()).collect();
    let n = criteria.len();
    for c in &mut criteria[n - relaxed.min(n)..] {
        *c = format!("{c} (lâché, sa place reste exigée)");
    }
    if let Some(filled) = filled_affixes(it).filter(|n| *n > 0) {
        criteria.push(format!("au moins {filled} mods, comme cet objet"));
    }
    if caster_weapon(it) && !base_only {
        criteria.push(format!("même base : {}", it.base));
    }
    if base_only {
        criteria.push(format!("base {} de niveau d'objet {} ou plus", it.base, it.item_level));
    }
    Ok(Estimate::Done { total, prices_ex, relaxed, url, criteria, instant, base_only })
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
        l.record(None, None);
        assert!(l.wait().is_zero());
        l.record(None, None);
        assert!(!l.wait().is_zero(), "marge d'une requête : la 3e doit attendre");
        // Le serveur compte aussi les requêtes des autres programmes.
        let mut l = Limiter::new(&[(5, 10), (30, 300)]);
        l.record(None, Some("1:10:0,29:300:0"));
        assert!(l.wait() > Duration::from_secs(250));
    }

    /// Tous les mods sont cherchés ; la recherche élargie lâche le plus faible mais garde sa
    /// place : un objet à 3 bons mods et un faible ne se compare pas à un objet à 3 bons mods
    /// et une place libre.
    #[test]
    fn weakest_mod_is_dropped_but_its_place_kept() {
        let text = "Item Class: Rings\nRarity: Rare\nDoom Loop\nRuby Ring\n--------\nItem Level: 80\n--------\n+60 to maximum Life\n+30% to Fire Resistance\n+30% to Cold Resistance\n+5 to Strength\n";
        let it = crate::item::parse(text).unwrap();
        let c = |id: &str, weight: f64| Criterion { id: id.into(), value: Some(1.0), label: id.into(), weight };
        let crit = [c("life", 3.0), c("res", 3.0), c("str", 2.0)];
        let ids = |q: &Value| -> Vec<String> { q["query"]["stats"][0]["filters"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap().to_string()).collect() };
        let strict = query(&it, &crit, 0, true);
        assert_eq!(ids(&strict), ["life", "res", "str"]);
        let relaxed = query(&it, &crit, 1, true);
        assert_eq!(ids(&relaxed), ["life", "res"]);
        let affixes = &relaxed["query"]["stats"][1]["filters"][0];
        assert_eq!((affixes["id"].as_str(), affixes["value"]["min"].as_u64()), (Some("pseudo.pseudo_number_of_affix_mods"), Some(4)));
        assert_eq!(strict["query"]["status"]["option"], "securable");
        assert_eq!(query(&it, &crit, 0, false)["query"]["status"]["option"], "online");
    }

    /// Aucun mod reconnu : on cherche la base, au moins du même niveau d'objet.
    #[test]
    fn no_mod_prices_the_base() {
        let text = "Item Class: Rings\nRarity: Rare\nDoom Loop\nRuby Ring\n--------\nItem Level: 80\n--------\n+5 to Strength\n";
        let it = crate::item::parse(text).unwrap();
        let q = query(&it, &[], 0, true);
        assert_eq!(q["query"]["type"], "Ruby Ring");
        assert_eq!(q["query"]["filters"]["misc_filters"]["filters"]["ilvl"]["min"], 80);
    }

    /// Un mod hybride (deux lignes) ne compte qu'une fois.
    #[test]
    fn hybrid_mod_is_one_affix() {
        let text = "Item Class: Staves\nRarity: Rare\nVortex Gnarl\nVoltaic Staff\n--------\nItem Level: 40\n--------\n{ Prefix Modifier \"Wizard's\" (Tier: 5) — Mana, Damage, Caster }\n57(49-58)% increased Spell Damage\n+53(49-56) to maximum Mana\n{ Suffix Modifier \"of the Augur\" (Tier: 5) — Attribute }\n+18(17-20) to Intelligence\n";
        let it = crate::item::parse(text).unwrap();
        assert_eq!(filled_affixes(&it), Some(2));
    }

    #[test]
    fn range_and_confidence() {
        let prices_ex = vec![2.0, 3.0, 4.0, 5.0, 6.0, 40.0];
        let e = Estimate::Done { total: 6, prices_ex, relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false };
        assert_eq!(e.range_ex(), Some((2.0, 6.0)));
        assert_eq!(e.confidence(), "bonne");
        let e = Estimate::Done { total: 3, prices_ex: vec![2.0, 3.0, 4.0], relaxed: 2, url: String::new(), criteria: vec![], instant: false, base_only: false };
        assert_eq!(e.confidence(), "faible");
    }

    #[test]
    fn median() {
        let e = Estimate::Done { total: 3, prices_ex: vec![5.0, 1.0, 3.0], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false };
        assert_eq!(e.value_ex(), Some(3.0));
        // Cas réel (arbalète niveau 11, recherche élargie) : les annonces hors de prix ne comptent pas.
        let prices_ex = vec![1.0, 1.0, 2.0, 4.0, 17.0, 666.0, 999.0, 1348.0, 2697.0, 74847.0];
        let e = Estimate::Done { total: 10, prices_ex, relaxed: 1, url: String::new(), criteria: vec![], instant: true, base_only: false };
        assert_eq!(e.value_ex(), Some(2.0));
        // Une annonce bradée ne fait pas tomber les autres.
        let e = Estimate::Done { total: 8, prices_ex: vec![1.0, 20.0, 25.0, 30.0, 40.0], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false };
        assert_eq!(e.value_ex(), Some(30.0));
        assert_eq!(e.range_ex(), Some((20.0, 40.0)));
        // Une annonce seule ne fait pas une estimation.
        let e = Estimate::Done { total: 1, prices_ex: vec![34905.0], relaxed: 1, url: String::new(), criteria: vec![], instant: true, base_only: false };
        assert_eq!(e.value_ex(), None);
    }
}

/// Estimations bloquantes à la suite, au rythme permis par l'API (limites partagées entre les
/// objets) : outil en ligne de commande `--price-all`.
pub fn estimate_all(items: &[Item], cache: &Path, league: &str, prices: &Prices, mut done: impl FnMut(&Item, Estimate)) {
    let index = StatIndex::load(cache);
    let mut search_limit = Limiter::new(&[(5, 10), (15, 60), (30, 300)]);
    let mut fetch_limit = Limiter::new(&[(12, 4), (16, 12), (50, 300)]);
    let league_path = league.replace(' ', "%20");
    for it in items {
        let e = estimate(it, &index, &mut search_limit, &mut fetch_limit, league, &league_path, prices, |m| eprintln!("… {m}")).unwrap_or_else(Estimate::Failed);
        done(it, e);
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
