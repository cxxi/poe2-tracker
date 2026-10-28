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
use crate::weights;

const API: &str = "https://www.pathofexile.com/api/trade2";
const SITE: &str = "https://www.pathofexile.com/trade2/search/poe2";
/// Tolérance sur les valeurs des mods recherchés.
const TOLERANCE: f64 = 0.85;
/// En dessous, on refait une recherche plus souple.
const ENOUGH: u64 = 5;
const FETCH_COUNT: usize = 30;
/// Écart de note toléré entre l'objet et une annonce comparable (±25 %).
pub const SCORE_GAP: f64 = 0.25;

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
    let cat = category(&it.item_class).unwrap_or("");
    let mut out = Vec::new();
    let (mut life, mut res, mut res_weight) = (0.0, 0.0, 0.0);
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
            res_weight += weights::contribution(cat, m);
            continue;
        }
        if let Some(c) = ALL_RES.captures(m) {
            res += 3.0 * c[1].parse::<f64>().unwrap_or(0.0);
            res_weight += weights::contribution(cat, m);
            continue;
        }
        let (tpl, value) = template(m);
        let Some(id) = index.get("explicit", &tpl) else { continue };
        // Ce que le mod pèse dans la note de l'objet : les plus légers sont lâchés en premier.
        let weight = weights::contribution(cat, m);
        out.push(Criterion { id: id.clone(), value, label: m.clone(), weight });
    }
    if life > 0.0 {
        out.push(Criterion { id: "pseudo.pseudo_total_life".into(), value: Some(life), label: format!("+{life} vie au total"), weight: weights::contribution(cat, &format!("+{life} to maximum Life")) });
    }
    if res > 0.0 {
        out.push(Criterion {
            id: "pseudo.pseudo_total_elemental_resistance".into(),
            value: Some(res),
            label: format!("+{res}% résistances élémentaires au total"),
            weight: res_weight,
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

/// Une annonce lue sur le site de trade.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Listing {
    pub name: String,
    pub base: String,
    pub item_level: u32,
    /// Prix demandé tel qu'annoncé (« 3 », « divine »), et en exalts.
    pub amount: f64,
    pub currency: String,
    pub ex: Option<f64>,
    /// Mods, implicites compris, sans les balises du site.
    pub mods: Vec<String>,
    /// Note de l'annonce (voir `weights`).
    #[serde(default)]
    pub score: f64,
    /// Retenue comme comparable (note proche de celle de l'objet).
    #[serde(default)]
    pub kept: bool,
    /// Âge de l'annonce à la lecture, en heures.
    #[serde(default)]
    pub age_hours: Option<f64>,
    /// Rareté (« Rare », « Magic »…), icône, et armure / évasion / bouclier d'énergie affichés.
    #[serde(default)]
    pub rarity: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub defences: [f64; 3],
}

/// Demi-vie du poids d'une annonce selon son âge : encore en ligne après quelques jours, elle
/// n'a pas trouvé preneur à ce prix et dit moins du marché du moment.
const AGE_HALF_LIFE_H: f64 = 72.0;
/// Poids minimal d'une vieille annonce.
const AGE_FLOOR: f64 = 0.15;

fn age_weight(age_hours: Option<f64>) -> f64 {
    age_hours.map(|h| 0.5f64.powf(h.max(0.0) / AGE_HALF_LIFE_H).max(AGE_FLOOR)).unwrap_or(1.0)
}

/// Date du site (« 2026-10-23T22:41:00Z ») en secondes Unix.
fn unix_time(date: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| date.get(r)?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    // Jours depuis le 1er janvier 1970 (calendrier grégorien).
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Ligne de mod du site de trade sans ses balises (« [Resistances|Cold Resistance] »).
pub fn clean_line(line: &str) -> String {
    let line = TAG_PAIR.replace_all(line, "$2");
    TAG.replace_all(&line, "$1").into_owned()
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
        /// Annonces lues, de la moins chère à la plus chère (pour les montrer).
        #[serde(default)]
        listings: Vec<Listing>,
        /// Note de l'objet estimé.
        #[serde(default)]
        score: f64,
        /// Trop peu d'annonces de note proche : les plus proches ont été prises.
        #[serde(default)]
        widened: bool,
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

/// Prix lu sur la tendance des annonces : valeur au niveau de la note de l'objet, moitié
/// centrale des prix ramenés à cette note, et nombre effectif d'annonces qui comptent.
struct Fit {
    value: f64,
    low: f64,
    high: f64,
    /// Annonces de note proche (±`SCORE_GAP`), comptées selon leur âge : une annonce neuve vaut
    /// 1, une vieille 0,15. C'est elle qui fonde la confiance, pas la somme des petits poids
    /// des annonces éloignées.
    near: f64,
    /// Part de chaque annonce dans le prix (même ordre que les annonces, 0 sans prix).
    shares: Vec<f64>,
    /// Écart (×) de la moitié centrale des prix proches, prix fantaisistes compris : un marché
    /// éparpillé ne doit pas paraître sûr parce que la passe robuste en a écarté une partie.
    spread: f64,
}

/// Quantile pondéré (`q` entre 0 et 1) de valeurs triées avec leurs poids.
fn weighted_quantile(points: &[(f64, f64)], q: f64) -> f64 {
    let total: f64 = points.iter().map(|(_, w)| w).sum();
    let mut acc = 0.0;
    for (v, w) in points {
        acc += w;
        if acc >= q * total {
            return *v;
        }
    }
    points.last().map(|(v, _)| *v).unwrap_or(0.0)
}

impl Estimate {
    /// Tendance du prix selon la note, sur toutes les annonces lues : chaque annonce pèse
    /// d'autant plus que sa note est proche de celle de l'objet (±`SCORE_GAP` : poids 0,6) et
    /// qu'elle est récente (demi-vie de `AGE_HALF_LIFE_H`), le
    /// prix suit la note (régression pondérée sur le logarithme du prix), et les prix ramenés à la
    /// note de l'objet donnent la valeur (médiane) et la fourchette (moitié centrale). Un prix
    /// fantaisiste isolé pèse peu ; une pente négative (bruit) est ramenée à zéro.
    fn fit(&self) -> Option<Fit> {
        let Estimate::Done { listings, score, .. } = self else { return None };
        if *score <= 0.0 {
            return None;
        }
        let mut index = Vec::new();
        let pts: Vec<(f64, f64, f64)> = listings
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let ex = l.ex.filter(|e| *e > 0.0)?;
                let d = (l.score / score - 1.0) / SCORE_GAP;
                index.push(i);
                Some((l.score - score, ex.ln(), (-0.5 * d * d).exp() * age_weight(l.age_hours)))
            })
            .collect();
        // Toujours la tendance dès qu'il y a assez d'annonces, même vieilles ou éloignées : c'est la
        // confiance qui le dit (l'ancien calcul, sans notes ni âge, donnerait un faux 100 %).
        let weight: f64 = pts.iter().map(|p| p.2).sum();
        if pts.len() < MIN_PRICED || weight <= 0.0 {
            return None;
        }
        let near: f64 = listings.iter().filter(|l| l.ex.is_some() && (l.score / score - 1.0).abs() <= SCORE_GAP).map(|l| age_weight(l.age_hours)).sum();
        // Régression pondérée y = a + b·x (x : écart de note), puis une seconde passe qui
        // écarte presque les annonces à plus de ×4 de la tendance.
        let mut w: Vec<f64> = pts.iter().map(|p| p.2).collect();
        let mut slope = 0.0;
        for pass in 0..2 {
            let sw: f64 = w.iter().sum();
            let mx = pts.iter().zip(&w).map(|(p, w)| w * p.0).sum::<f64>() / sw;
            let my = pts.iter().zip(&w).map(|(p, w)| w * p.1).sum::<f64>() / sw;
            let sxx: f64 = pts.iter().zip(&w).map(|(p, w)| w * (p.0 - mx).powi(2)).sum();
            let sxy: f64 = pts.iter().zip(&w).map(|(p, w)| w * (p.0 - mx) * (p.1 - my)).sum();
            slope = if sxx > 1e-9 { (sxy / sxx).max(0.0) } else { 0.0 };
            if pass == 0 {
                let a = my - slope * mx;
                for (wi, p) in w.iter_mut().zip(&pts) {
                    if (p.1 - (a + slope * p.0)).abs() > 4f64.ln() {
                        *wi *= 0.1;
                    }
                }
            }
        }
        // Prix ramenés à la note de l'objet.
        let ramp = |w: &[f64]| {
            let mut v: Vec<(f64, f64)> = pts.iter().zip(w).map(|(p, w)| (p.1 - slope * p.0, *w)).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v
        };
        let adjusted = ramp(&w);
        let raw = ramp(&pts.iter().map(|p| p.2).collect::<Vec<_>>());
        let total: f64 = w.iter().sum();
        let mut shares = vec![0.0; listings.len()];
        for (k, i) in index.iter().enumerate() {
            shares[*i] = w[k] / total;
        }
        Some(Fit {
            value: weighted_quantile(&adjusted, 0.5).exp(),
            low: weighted_quantile(&adjusted, 0.25).exp(),
            high: weighted_quantile(&adjusted, 0.75).exp(),
            near,
            shares,
            spread: (weighted_quantile(&raw, 0.75) - weighted_quantile(&raw, 0.25)).exp(),
        })
    }

    /// Prix retenus (estimations d'avant les notes) : les `VALUE_FROM` annonces les moins
    /// chères, sans les valeurs aberrantes (plus de 5 fois au-dessus ou en dessous de leur
    /// médiane).
    fn kept(&self) -> Vec<f64> {
        let Estimate::Done { prices_ex, .. } = self else { return vec![] };
        let mut p = prices_ex.clone();
        p.sort_by(f64::total_cmp);
        p.truncate(VALUE_FROM);
        let Some(mid) = p.get(p.len() / 2).copied() else { return p };
        p.retain(|v| *v <= mid.max(0.1) * OUTLIER && *v >= mid / OUTLIER);
        p
    }

    /// Valeur estimée en exalts : prix de la tendance au niveau de la note de l'objet (sinon,
    /// médiane des annonces retenues s'il y en a au moins `MIN_PRICED`).
    pub fn value_ex(&self) -> Option<f64> {
        if let Some(f) = self.fit() {
            return Some(f.value);
        }
        let p = self.kept();
        if p.len() < MIN_PRICED {
            return None;
        }
        p.get(p.len() / 2).copied()
    }

    /// Fourchette : moitié centrale des prix ramenés à la note de l'objet (sinon, la moins chère
    /// et la plus chère des annonces retenues).
    pub fn range_ex(&self) -> Option<(f64, f64)> {
        if let Some(f) = self.fit() {
            return Some((f.low, f.high));
        }
        let p = self.kept();
        (p.len() >= MIN_PRICED).then(|| (p[0], p[p.len() - 1]))
    }

    /// Confiance dans l'estimation, en pourcentage : 100 % avec au moins 5 annonces récentes de
    /// note proche, des prix serrés et tous les mods cherchés. Moins 15 points par annonce proche
    /// en moins (une vieille annonce ne compte que pour 0,15), moins 15 par mod lâché, et selon l'écart de la fourchette : rien jusqu'à ×2, puis
    /// 25 points par doublement (×4 : −25, ×8 : −50…).
    pub fn confidence_pct(&self) -> u32 {
        let Estimate::Done { relaxed, instant, widened, .. } = self else { return 0 };
        let mut pct = match self.fit() {
            Some(f) => {
                let spread = f.spread.max(1.0);
                100.0 - 15.0 * (VALUE_FROM as f64 - f.near).max(0.0) - 25.0 * (spread / 2.0).log2().max(0.0)
            }
            None => {
                let kept = self.kept().len().min(VALUE_FROM) as f64;
                if (kept as usize) < MIN_PRICED {
                    return 0;
                }
                let mut pct = 100.0 - 10.0 * (VALUE_FROM as f64 - kept);
                if *widened {
                    pct -= 20.0;
                }
                pct
            }
        } as i32;
        pct -= 15 * *relaxed as i32;
        if !instant {
            pct -= 20;
        }
        pct.clamp(10, 100) as u32
    }

    /// Part de chaque annonce dans le prix (même ordre que les annonces), si le prix vient de la
    /// tendance.
    pub fn shares(&self) -> Option<Vec<f64>> {
        self.fit().map(|f| f.shares)
    }

    /// Annonces de note proche, comptées selon leur âge (voir `Fit::near`).
    #[cfg(test)]
    pub fn near(&self) -> Option<f64> {
        self.fit().map(|f| f.near)
    }

    /// Écart (×) des prix des annonces proches, pour expliquer la confiance.
    pub fn spread(&self) -> Option<f64> {
        self.fit().map(|f| f.spread)
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
    /// Estimations relancées alors qu'il y en avait déjà une : l'ancienne reste affichée jusqu'à
    /// la nouvelle (texte de l'objet -> étape en cours).
    refreshing: Arc<Mutex<HashMap<String, String>>>,
}

impl Trade {
    pub fn start(ctx: egui::Context, cache: PathBuf, league: String, prices: Prices) -> Trade {
        let (tx, rx) = channel::<Item>();
        let saved = load_saved(&cache, &league).into_iter().map(|(raw, (_, e))| (raw, e)).collect();
        let results: Shared = Arc::new(Mutex::new(saved));
        let refreshing: Arc<Mutex<HashMap<String, String>>> = Arc::default();
        let (shared, again) = (results.clone(), refreshing.clone());
        std::thread::spawn(move || worker(rx, shared, again, ctx, cache, league, prices));
        Trade { tx, results, refreshing }
    }

    pub fn request(&self, it: &Item) {
        let mut r = self.results.lock().unwrap();
        if matches!(r.get(&it.raw), Some(Estimate::Done { .. })) {
            return; // déjà estimé
        }
        r.insert(it.raw.clone(), Estimate::Pending("en file d'attente".into()));
        let _ = self.tx.send(it.clone());
    }

    /// Relance l'estimation même si elle a déjà été faite (les prix bougent). L'estimation
    /// faite reste affichée jusqu'à ce que la nouvelle la remplace d'un coup.
    pub fn request_again(&self, it: &Item) {
        if matches!(self.get(it), Some(Estimate::Done { .. })) {
            self.refreshing.lock().unwrap().insert(it.raw.clone(), "en file d'attente".into());
        } else {
            self.results.lock().unwrap().insert(it.raw.clone(), Estimate::Pending("en file d'attente".into()));
        }
        let _ = self.tx.send(it.clone());
    }

    /// Estimation relancée en cours pour cet objet : son étape (« recherche… »).
    pub fn refreshing(&self, it: &Item) -> Option<String> {
        self.refreshing.lock().unwrap().get(&it.raw).cloned()
    }

    pub fn get(&self, it: &Item) -> Option<Estimate> {
        self.results.lock().unwrap().get(&it.raw).cloned()
    }
}

fn set(shared: &Shared, ctx: &egui::Context, key: &str, e: Estimate) {
    shared.lock().unwrap().insert(key.to_string(), e);
    ctx.request_repaint();
}

fn worker(rx: Receiver<Item>, shared: Shared, refreshing: Arc<Mutex<HashMap<String, String>>>, ctx: egui::Context, cache: PathBuf, league: String, prices: Prices) {
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
                // Une estimation relancée garde l'ancienne.
                if refreshing.lock().unwrap().remove(&old.raw).is_none() {
                    set(&shared, &ctx, &old.raw, Estimate::Failed("ignoré : trop d'objets copiés d'affilée".into()));
                }
            }
        }
        while let Some(it) = queue.pop_back() {
            let result = estimate(&it, &index, &mut search_limit, &mut fetch_limit, &league, &league_path, &prices, |msg| {
                let mut again = refreshing.lock().unwrap();
                match again.get_mut(&it.raw) {
                    Some(step) => {
                        *step = msg;
                        ctx.request_repaint();
                    }
                    None => {
                        drop(again);
                        set(&shared, &ctx, &it.raw, Estimate::Pending(msg));
                    }
                }
            });
            if let Ok(e) = &result {
                save(&cache, &league, &it.raw, e);
            }
            // Relance qui échoue : l'estimation d'avant reste.
            let again = refreshing.lock().unwrap().remove(&it.raw).is_some();
            match result {
                Ok(e) => set(&shared, &ctx, &it.raw, e),
                Err(e) if !again => set(&shared, &ctx, &it.raw, Estimate::Failed(e)),
                Err(_) => ctx.request_repaint(),
            }
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

/// Fin de l'attente imposée par les limites de l'API, pour le décompte affiché.
static WAIT_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Temps restant avant la prochaine requête, si les limites de l'API imposent d'attendre.
pub fn wait_left() -> Option<Duration> {
    let until = (*WAIT_UNTIL.lock().unwrap())?;
    let left = until.saturating_duration_since(Instant::now());
    (!left.is_zero()).then_some(left)
}

fn throttle(limit: &Limiter, report: &dyn Fn(String)) {
    let wait = limit.wait();
    if !wait.is_zero() {
        *WAIT_UNTIL.lock().unwrap() = Some(Instant::now() + wait);
        report("limite de l'API de trade : attente".into());
        std::thread::sleep(wait);
        *WAIT_UNTIL.lock().unwrap() = None;
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
    let cat = category(&it.item_class).unwrap_or("");
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
    let mut found = Vec::new();
    // Le site rend 10 annonces par lecture : jusqu'à 3 lectures (limite bien plus large que celle
    // des recherches).
    for (n, chunk) in hashes.chunks(10).enumerate() {
        report(if n == 0 { "lecture des annonces…".into() } else { format!("lecture des annonces ({}/{})…", n + 1, hashes.len().div_ceil(10)) });
        let fetch_url = format!("{API}/fetch/{}?query={id}", chunk.join(","));
        let listings = send(fetch_limit, &report, || ureq::get(&fetch_url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call())?;
        let data = prices.data.lock().unwrap();
        for r in listings["result"].as_array().into_iter().flatten() {
            let price = &r["listing"]["price"];
            let (amount, cur) = (price["amount"].as_f64(), price["currency"].as_str());
            let ex = match (amount, cur, data.as_ref()) {
                (Some(amount), Some(cur), Some(d)) => d.listing_to_ex(amount, cur),
                _ => None,
            };
            let item = &r["item"];
            // Chaque mod est un objet { description, … } (ou un simple texte).
            let mods: Vec<String> = ["enchantMods", "runeMods", "implicitMods", "fracturedMods", "explicitMods", "desecratedMods"]
                .iter()
                .flat_map(|k| item[*k].as_array().into_iter().flatten())
                .filter_map(|m| m["description"].as_str().or(m.as_str()).map(clean_line))
                .collect();
            found.push(Listing {
                name: item["name"].as_str().unwrap_or_default().to_string(),
                base: item["typeLine"].as_str().unwrap_or_default().to_string(),
                item_level: item["ilvl"].as_u64().unwrap_or(0) as u32,
                amount: amount.unwrap_or(0.0),
                currency: cur.unwrap_or_default().to_string(),
                ex,
                score: weights::score(cat, &mods),
                kept: false,
                age_hours: r["listing"]["indexed"].as_str().and_then(unix_time).map(|t| (now_secs() as i64 - t) as f64 / 3600.0),
                rarity: item["rarity"].as_str().unwrap_or_default().to_string(),
                icon: item["icon"].as_str().unwrap_or_default().to_string(),
                defences: ["ar", "ev", "es"].map(|k| item["extended"][k].as_f64().unwrap_or(0.0)),
                mods,
            });
        }
    }
    // Annonces proches : note à ±`SCORE_GAP` de celle de l'objet.
    let score = weights::score(cat, &it.mods);
    let gap = |l: &Listing| if score > 0.0 { (l.score / score - 1.0).abs() } else { 0.0 };
    for l in &mut found {
        l.kept = l.ex.is_some() && gap(l) <= SCORE_GAP;
    }
    // Le prix vient de la tendance sur toutes les annonces (voir `Estimate::fit`) ; `kept`
    // marque seulement les plus proches, pour les montrer.
    let widened = false;
    let prices_ex: Vec<f64> = found.iter().filter_map(|l| l.ex).collect();
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
    Ok(Estimate::Done { total, prices_ex, relaxed, url, criteria, instant, base_only, listings: found, score, widened })
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
        let e = Estimate::Done { total: 6, prices_ex, relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false, listings: vec![], score: 0.0, widened: false };
        assert_eq!(e.range_ex(), Some((2.0, 6.0)));
        assert_eq!(e.confidence(), "bonne");
        let e = Estimate::Done { total: 3, prices_ex: vec![2.0, 3.0, 4.0], relaxed: 2, url: String::new(), criteria: vec![], instant: false, base_only: false, listings: vec![], score: 0.0, widened: false };
        assert_eq!(e.confidence(), "faible");
    }

    #[test]
    fn trend_over_scores() {
        let l = |score: f64, ex: f64| Listing { score, ex: Some(ex), ..Default::default() };
        // Prix qui doublent quand la note monte de 2, plus un prix fantaisiste isolé.
        let listings = vec![l(8.0, 5.0), l(9.0, 7.0), l(10.0, 10.0), l(11.0, 14.0), l(12.0, 20.0), l(10.0, 500.0), l(10.2, 10.5)];
        let e = Estimate::Done { total: 7, prices_ex: vec![], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false, listings, score: 10.0, widened: false };
        let v = e.value_ex().unwrap();
        assert!((8.0..13.0).contains(&v), "{v}");
        let (lo, hi) = e.range_ex().unwrap();
        assert!(lo <= v && v <= hi && hi / lo < 2.5, "{lo} {hi}");
    }

    #[test]
    fn listing_dates() {
        assert_eq!(unix_time("1970-01-02T00:00:00Z"), Some(86_400));
        assert_eq!(unix_time("2026-10-23T22:41:00Z"), Some(1_792_795_260));
        assert_eq!(age_weight(Some(72.0)), 0.5);
        assert_eq!(age_weight(Some(24.0 * 30.0)), AGE_FLOOR);
    }

    #[test]
    fn old_far_listings_still_use_the_trend() {
        // Cas réel (carquois) : une seule annonce proche, vieille de 52 jours ; les autres plus
        // éloignées et vieilles aussi. Le prix vient quand même de la tendance, et la confiance
        // s'effondre (avant : médiane des moins chères, 100 %).
        let l = |score: f64, ex: f64, days: f64| Listing { score, ex: Some(ex), age_hours: Some(days * 24.0), kept: (score / 7.0 - 1.0).abs() <= SCORE_GAP, ..Default::default() };
        let listings = vec![l(8.3, 139.0, 52.0), l(9.7, 1.0, 43.0), l(10.8, 1.0, 44.0), l(10.4, 5.0, 3.0), l(8.9, 69.0, 52.0), l(9.1, 139.0, 53.0)];
        let e = Estimate::Done { total: 34, prices_ex: vec![139.0, 1.0, 1.0, 5.0, 69.0, 139.0], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false, listings, score: 7.0, widened: false };
        assert!(e.near().is_some());
        assert!(e.confidence_pct() < 30, "{}", e.confidence_pct());
    }

    #[test]
    fn spread_lowers_confidence() {
        let l = |score: f64, ex: f64| Listing { score, ex: Some(ex), ..Default::default() };
        let tight = vec![l(10.0, 10.0), l(10.0, 11.0), l(10.0, 9.0), l(10.0, 10.0), l(10.0, 12.0)];
        let loose = vec![l(10.0, 2.0), l(10.0, 7.0), l(10.0, 70.0), l(10.0, 140.0), l(10.0, 15.0)];
        let done = |listings| Estimate::Done { total: 5, prices_ex: vec![], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false, listings, score: 10.0, widened: false };
        assert!(done(tight).confidence_pct() >= 90);
        assert!(done(loose).confidence_pct() < 50);
    }

    #[test]
    fn median() {
        let e = Estimate::Done { total: 3, prices_ex: vec![5.0, 1.0, 3.0], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false, listings: vec![], score: 0.0, widened: false };
        assert_eq!(e.value_ex(), Some(3.0));
        // Cas réel (arbalète niveau 11, recherche élargie) : les annonces hors de prix ne comptent pas.
        let prices_ex = vec![1.0, 1.0, 2.0, 4.0, 17.0, 666.0, 999.0, 1348.0, 2697.0, 74847.0];
        let e = Estimate::Done { total: 10, prices_ex, relaxed: 1, url: String::new(), criteria: vec![], instant: true, base_only: false, listings: vec![], score: 0.0, widened: false };
        assert_eq!(e.value_ex(), Some(2.0));
        // Une annonce bradée ne fait pas tomber les autres.
        let e = Estimate::Done { total: 8, prices_ex: vec![1.0, 20.0, 25.0, 30.0, 40.0], relaxed: 0, url: String::new(), criteria: vec![], instant: true, base_only: false, listings: vec![], score: 0.0, widened: false };
        assert_eq!(e.value_ex(), Some(30.0));
        assert_eq!(e.range_ex(), Some((20.0, 40.0)));
        // Une annonce seule ne fait pas une estimation.
        let e = Estimate::Done { total: 1, prices_ex: vec![34905.0], relaxed: 1, url: String::new(), criteria: vec![], instant: true, base_only: false, listings: vec![], score: 0.0, widened: false };
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
