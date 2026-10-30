//! Guide de build : étapes lues dans les fichiers .build du planificateur du jeu
//! (Act 1, Act 2… Endgame Step 3), avec les noms lisibles des gemmes et passifs.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::art::USER_AGENT;
use crate::gear::Slot;

const POB: &str = "https://raw.githubusercontent.com/PathOfBuildingCommunity/PathOfBuilding-PoE2/dev/src";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Passive {
    pub name: String,
    pub notable: bool,
    pub ascendancy: bool,
    #[serde(default)]
    pub keystone: bool,
    #[serde(default)]
    pub jewel: bool,
    #[serde(default)]
    pub stats: Vec<String>,
    /// Position dans l'arbre (coordonnées PoB) et nœuds reliés (identifiants numériques).
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub skill: u32,
    #[serde(default)]
    pub links: Vec<u32>,
    /// Nom de l'ascendance pour ses nœuds.
    #[serde(default)]
    pub ascendancy_name: Option<String>,
    /// Icône dans les archives du jeu ("Art/2DArt/SkillIcons/passives/….dds").
    #[serde(default)]
    pub icon: String,
    /// Nœud de départ d'une classe (pris d'office, absent des fichiers .build).
    #[serde(default)]
    pub class_start: bool,
    /// Centre du groupe et couronne du nœud (liaisons qui suivent le cercle du groupe).
    #[serde(default)]
    pub gx: f32,
    #[serde(default)]
    pub gy: f32,
    #[serde(default)]
    pub orbit: u8,
    /// Pour chaque liaison de `links` : rayon de l'arc (signé selon le côté), 0 = droite.
    #[serde(default)]
    pub link_arcs: Vec<f32>,
    /// Pas un vrai nœud à afficher : décor (isOnlyImage), sous-choix, emplacement « Sinister ».
    #[serde(default)]
    pub hidden: bool,
    /// Nœud « +5 à un attribut au choix ».
    #[serde(default)]
    pub attribute: bool,
}

/// Noms lisibles des passifs (stringId) et des gemmes (fin du gameId).
#[derive(Default, Serialize, Deserialize)]
pub struct Catalog {
    pub passives: HashMap<String, Passive>,
    pub gems: HashMap<String, String>,
    /// Prérequis Force / Dextérité / Intelligence des gemmes, par nom.
    #[serde(default)]
    pub gem_reqs: HashMap<String, [u32; 3]>,
    /// Centre de l'illustration de chaque ascendance dans l'arbre (coordonnées PoB).
    #[serde(default)]
    pub ascendancy_centers: HashMap<String, [f32; 2]>,
    /// Texte des uniques (PoB), par nom : type de base, implicites, mods.
    #[serde(default)]
    pub uniques: HashMap<String, UniqueText>,
    /// Effet de chaque charme pendant son utilisation (PoB : « 15% increased Rarity… »).
    #[serde(default)]
    pub charm_buffs: HashMap<String, Vec<String>>,
}

static BASE_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^itemBases\["([^"]+)"\] = \{"#).unwrap());
static CHARM_BUFF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"buff = \{([^}]*)\}").unwrap());
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]*)""#).unwrap());

/// Effets des charmes dans `Data/Bases/flask.lua` de PoB.
pub fn parse_charm_buffs(lua: &str) -> HashMap<String, Vec<String>> {
    let starts: Vec<(usize, String)> = BASE_START.captures_iter(lua).map(|c| (c.get(0).unwrap().start(), c[1].to_string())).collect();
    let mut out = HashMap::new();
    for (k, (at, name)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(lua.len(), |(e, _)| *e);
        if let Some(c) = CHARM_BUFF.captures(&lua[*at..end]) {
            let lines: Vec<String> = QUOTED.captures_iter(&c[1]).map(|q| q[1].to_string()).collect();
            if !lines.is_empty() {
                out.insert(name.clone(), lines);
            }
        }
    }
    out
}

/// Un unique tel que PoB le décrit (version actuelle), plages de valeurs comprises.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UniqueText {
    pub base: String,
    pub implicits: Vec<String>,
    pub mods: Vec<String>,
}

/// Fichiers d'uniques de PoB (`Data/Uniques/<nom>.lua`).
const UNIQUE_FILES: &[&str] = &[
    "amulet", "axe", "belt", "body", "boots", "bow", "claw", "crossbow", "dagger", "flail", "flask", "focus", "gloves", "helmet",
    "jewel", "mace", "quiver", "ring", "sceptre", "shield", "soulcore", "spear", "staff", "sword", "talisman", "wand",
];

/// Lignes d'en-tête d'un unique PoB (pas des mods).
const UNIQUE_META: &[&str] = &[
    "League:", "Source:", "Variant:", "Version:", "Selected Variant:", "Selected Version:", "LevelReq:", "Requires ", "Implicits:",
    "Has Alt Variant", "Radius:", "Limited to:", "Sockets:", "Item Level:", "Quality:", "Upgrade:",
];

static UNIQUE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\{(\w+):([^}]*)\}").unwrap());

/// Uniques d'un fichier PoB : blocs `[[ … ]]` (nom, base, en-tête, puis mods). Les mods marqués
/// `{version:…}` ne sont gardés que pour la dernière version, de même pour `{variant:…}` quand
/// une variante s'appelle « Current » ; les autres marques (`{tags:…}`, `{range:…}`) sont retirées.
pub fn parse_uniques(lua: &str) -> Vec<(String, UniqueText)> {
    let mut out = Vec::new();
    for block in lua.split("[[").skip(1) {
        let Some(body) = block.split("]]").next() else { continue };
        let mut lines = body.lines().map(str::trim).filter(|l| !l.is_empty());
        let (Some(name), Some(base)) = (lines.next(), lines.next()) else { continue };
        let (mut versions, mut variants, mut implicits) = (0usize, Vec::new(), 0usize);
        let mut raw = Vec::new();
        for l in lines {
            if raw.is_empty() && UNIQUE_META.iter().any(|m| l.starts_with(m)) {
                if l.starts_with("Version:") {
                    versions += 1;
                } else if let Some(v) = l.strip_prefix("Variant:") {
                    variants.push(v.trim().to_string());
                } else if let Some(n) = l.strip_prefix("Implicits:") {
                    implicits = n.trim().parse().unwrap_or(0);
                }
                continue;
            }
            raw.push(l);
        }
        let current_variant = variants.iter().position(|v| v == "Current").map(|i| i + 1);
        let mut text = UniqueText { base: base.to_string(), ..Default::default() };
        // Variantes au choix (pas de « Current ») : leurs lignes, fusionnées après coup.
        let mut choices: Vec<(usize, String)> = Vec::new();
        for (i, l) in raw.iter().enumerate() {
            let mut line: &str = l;
            let mut keep = true;
            let mut choice = false;
            while let Some(c) = UNIQUE_TAG.captures(line) {
                let wanted = |n: usize| c[2].split(',').any(|v| v.trim().parse::<usize>().ok() == Some(n));
                match &c[1] {
                    "version" if versions > 0 => keep &= wanted(versions),
                    "variant" => {
                        keep &= current_variant.is_none_or(wanted);
                        choice |= current_variant.is_none();
                    }
                    _ => {}
                }
                line = &line[c[0].len()..];
            }
            if keep && !line.is_empty() {
                if choice && i >= implicits {
                    choices.push((text.mods.len(), line.to_string()));
                } else if i < implicits {
                    text.implicits.push(line.to_string())
                } else {
                    text.mods.push(line.to_string())
                }
            }
        }
        // Une ligne par variante au choix, qui ne diffèrent que d'un mot ou deux : une seule
        // ligne, le passage qui change remplacé par « [au choix] ».
        if let Some(at) = choices.first().map(|(a, _)| *a) {
            let words: Vec<Vec<&str>> = choices.iter().map(|(_, l)| l.split(' ').collect()).collect();
            let shortest = words.iter().map(Vec::len).min().unwrap_or(0);
            let head = (0..shortest).take_while(|k| words.iter().all(|w| w[*k] == words[0][*k])).count();
            let tail = (0..shortest - head).take_while(|k| words.iter().all(|w| w[w.len() - 1 - k] == words[0][words[0].len() - 1 - k])).count();
            if choices.len() > 1 && head + tail > 0 {
                let w = &words[0];
                let merged = [&w[..head], &["[au choix]"], &w[w.len() - tail..]].concat().join(" ");
                // Rite of Passage : PoB liste un esprit par variante, mais en jeu il est tiré au
                // hasard à chaque utilisation (« Possessed by a random Spirit… »).
                let merged = merged.replace("Spirit Of The [au choix]", "a random Spirit");
                text.mods.insert(at.min(text.mods.len()), merged);
            } else {
                for (k, (_, l)) in choices.into_iter().enumerate() {
                    text.mods.insert((at + k).min(text.mods.len()), l);
                }
            }
        }
        out.push((name.to_string(), text));
    }
    out
}

static GEM_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\t\["Metadata/[^"]+"\] = \{$"#).unwrap());
static GEM_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\t\tname = "([^"]*)",$"#).unwrap());
static GEM_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\t\tgameId = "([^"]*)",$"#).unwrap());
static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([a-z])([A-Z])").unwrap());

fn get_text(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(ureq::get(url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(60)).call()?.into_string()?)
}

fn key(game_id: &str) -> &str {
    game_id.rsplit('/').next().unwrap_or(game_id)
}

impl Catalog {
    fn download() -> Result<Catalog, Box<dyn std::error::Error>> {
        let mut cat = Catalog::default();
        let tree: serde_json::Value = serde_json::from_str(&get_text(&format!("{POB}/TreeData/0_5/tree.json"))?)?;
        let radii: Vec<f64> = tree["constants"]["orbitRadii"].as_array().into_iter().flatten().filter_map(|v| v.as_f64()).collect();
        let angles: Vec<Vec<f64>> = tree["constants"]["orbitAnglesByOrbit"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| o.as_array().into_iter().flatten().filter_map(|v| v.as_f64()).collect())
            .collect();
        for asc in tree["classes"].as_array().into_iter().flatten().flat_map(|c| c["ascendancies"].as_array().into_iter().flatten()) {
            let bg = &asc["background"];
            if let (Some(name), Some(x), Some(y)) = (asc["name"].as_str(), bg["x"].as_f64(), bg["y"].as_f64()) {
                cat.ascendancy_centers.insert(name.to_string(), [x as f32, y as f32]);
            }
        }
        for node in tree["nodes"].as_object().into_iter().flat_map(|o| o.values()) {
            let Some(id) = node["stringId"].as_str() else { continue };
            // Position : centre du groupe + rayon de l'orbite, à l'angle de l'emplacement (convention PoB).
            // Les groupes forment une liste ; leurs numéros commencent à 1.
            let Some(gi) = node["group"].as_u64().and_then(|g| g.checked_sub(1)) else { continue };
            let group = &tree["groups"][gi as usize];
            if group.is_null() {
                continue;
            }
            let (gx, gy) = (group["x"].as_f64().unwrap_or(0.0), group["y"].as_f64().unwrap_or(0.0));
            let orbit = node["orbit"].as_u64().unwrap_or(0) as usize;
            let index = node["orbitIndex"].as_u64().unwrap_or(0) as usize;
            let r = radii.get(orbit).copied().unwrap_or(0.0);
            let a = angles.get(orbit).and_then(|o| o.get(index)).copied().unwrap_or(0.0);
            let keystone = node["isKeystone"].as_bool().unwrap_or(false);
            cat.passives.insert(id.to_string(), Passive {
                name: node["name"].as_str().unwrap_or(id).to_string(),
                notable: node["isNotable"].as_bool().unwrap_or(false) || keystone,
                ascendancy: node["ascendancyName"].is_string(),
                keystone,
                jewel: node["isJewelSocket"].as_bool().unwrap_or(false),
                stats: node["stats"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect(),
                x: (gx + r * a.sin()) as f32,
                y: (gy - r * a.cos()) as f32,
                skill: node["skill"].as_u64().unwrap_or(0) as u32,
                links: node["connections"].as_array().into_iter().flatten().filter_map(|c| c["id"].as_u64().map(|v| v as u32)).collect(),
                link_arcs: node["connections"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| c["id"].as_u64().is_some())
                    .map(|c| {
                        // « orbit » : indice du rayon, signé ; hors table (ex. 2147483647) = droite.
                        let o = c["orbit"].as_i64().unwrap_or(0);
                        match radii.get(o.unsigned_abs() as usize) {
                            Some(r) if o != 0 => (*r as f32).copysign(o as f32),
                            _ => 0.0,
                        }
                    })
                    .collect(),
                gx: gx as f32,
                gy: gy as f32,
                orbit: orbit as u8,
                ascendancy_name: node["ascendancyName"].as_str().map(String::from),
                icon: node["icon"].as_str().unwrap_or_default().to_string(),
                class_start: node["classesStart"].as_array().is_some_and(|a| !a.is_empty()),
                hidden: ["isOnlyImage", "isMultipleChoiceOption", "sinister"].iter().any(|k| node[*k].as_bool().unwrap_or(false)),
                attribute: node["isAttribute"].as_bool().unwrap_or(false),
            });
        }
        if let Ok(lua) = get_text(&format!("{POB}/Data/Bases/flask.lua")) {
            cat.charm_buffs = parse_charm_buffs(&lua);
        }
        // Uniques : un fichier par type d'objet ; un fichier manquant n'empêche pas le reste.
        for f in UNIQUE_FILES {
            if let Ok(lua) = get_text(&format!("{POB}/Data/Uniques/{f}.lua")) {
                cat.uniques.extend(parse_uniques(&lua));
            }
        }
        let lua = get_text(&format!("{POB}/Data/Gems.lua"))?;
        let starts: Vec<usize> = GEM_START.find_iter(&lua).map(|m| m.start()).chain([lua.len()]).collect();
        for w in starts.windows(2) {
            let block = &lua[w[0]..w[1]];
            if let (Some(n), Some(id)) = (GEM_NAME.captures(block), GEM_ID.captures(block)) {
                cat.gems.entry(key(&id[1]).to_string()).or_insert_with(|| n[1].to_string());
                let req = |k: &str| {
                    Regex::new(&format!(r"(?m)^\t\t{k} = (\d+),$")).ok().and_then(|r| r.captures(block)).and_then(|c| c[1].parse().ok()).unwrap_or(0)
                };
                cat.gem_reqs.entry(n[1].to_string()).or_insert([req("reqStr"), req("reqDex"), req("reqInt")]);
            }
        }
        Ok(cat)
    }

    /// Depuis le cache (une semaine), sinon téléchargé ; vide si hors ligne.
    pub fn load(cache: &Path) -> Catalog {
        let path = cache.join("pob_catalog_v11.json");
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < Duration::from_secs(7 * 86400))
            .unwrap_or(false);
        let cached = || std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
        if fresh {
            if let Some(c) = cached() {
                return c;
            }
        }
        match Catalog::download() {
            Ok(c) => {
                let _ = std::fs::create_dir_all(cache);
                let _ = std::fs::write(&path, serde_json::to_string(&c).unwrap_or_default());
                c
            }
            Err(e) => {
                eprintln!("noms des gemmes/passifs indisponibles : {e}");
                cached().unwrap_or_default()
            }
        }
    }

    /// Attribut dominant (0 Force, 1 Dextérité, 2 Intelligence) d'après les gemmes de l'étape.
    pub fn main_attribute(&self, step: &Step) -> usize {
        let mut total = [0u32; 3];
        for s in &step.skills {
            for name in std::iter::once(&s.name).chain(s.supports.iter().map(|(n, _)| n)) {
                if let Some(r) = self.gem_reqs.get(name) {
                    for i in 0..3 {
                        total[i] += r[i];
                    }
                }
            }
        }
        (0..3).max_by_key(|i| total[*i]).unwrap_or(2)
    }

    pub fn gem(&self, game_id: &str) -> String {
        let k = key(game_id);
        self.gems.get(k).cloned().unwrap_or_else(|| {
            // Repli : "SkillGemIceNova" -> "Ice Nova"
            let bare = k.trim_start_matches("SkillGem").trim_start_matches("SupportGem");
            CAMEL.replace_all(bare, "$1 $2").into_owned()
        })
    }
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub from_level: u32,
    pub supports: Vec<(String, u32)>,
    /// Set d'armes où le placer (1 ou 2 ; 0 : les deux), quand le build le dit.
    pub weapon_set: Option<u8>,
}

impl Skill {
    pub fn describe(&self) -> String {
        if self.supports.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.supports.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))
        }
    }
}

/// Mod visé par le guide pour un emplacement : modèle de stat ("# to maximum Life"), valeur visée.
#[derive(Clone, Debug)]
pub struct Target {
    pub template: String,
    pub value: f64,
    pub line: String,
}

#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub skills: Vec<Skill>,
    pub passives: Vec<String>,
    /// Équipement visé : emplacement -> (base visée, mods visés).
    pub targets: Vec<(Slot, String, Vec<Target>)>,
    /// Uniques recommandés à cette étape.
    pub uniques: Vec<Wanted>,
    /// Passifs propres à un set d'armes (1 ou 2) ; les autres sont communs aux deux.
    pub passive_sets: HashMap<String, u8>,
    /// Fioles et charmes visés : à part de `targets`, ils ne comptent ni dans le profil du
    /// build ni dans les caractéristiques, seulement dans la fiche de leur case.
    pub side_targets: Vec<(Slot, String, Vec<Target>)>,
}

/// Unique recommandé par le build. Le planificateur range toutes les fioles sous `Flask1` :
/// une fiole de mana arrive ici en fiole de vie (voir `uniques::slot`).
#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    pub name: String,
    pub slot: Option<Slot>,
    /// Place dans la rangée du planificateur (charmes : 0 à 2 ; fioles : 0 vie, 1 mana).
    pub position: usize,
}

impl Step {
    pub fn targets_for(&self, slot: Slot) -> impl Iterator<Item = &Target> {
        self.targets.iter().chain(&self.side_targets).filter(move |(s, _, _)| *s == slot).flat_map(|(_, _, t)| t)
    }

    /// Type de base visé sur `slot` (vide si le build n'en précise pas).
    pub fn base_for(&self, slot: Slot) -> &str {
        self.targets.iter().chain(&self.side_targets).find(|(s, b, _)| *s == slot && !b.is_empty()).map(|(_, b, _)| b.as_str()).unwrap_or_default()
    }

    pub fn all_targets(&self) -> impl Iterator<Item = &Target> {
        self.targets.iter().flat_map(|(_, _, t)| t)
    }
}

static TARGET_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\s*(.+)$").unwrap());

/// "Weapon1" -> arme… (les fioles et charmes ont leur propre comparaison).
fn slot_of(inventory_id: &str) -> Option<Slot> {
    Some(match inventory_id {
        "Weapon1" => Slot::Weapon,
        "Offhand1" => Slot::Offhand,
        "Weapon2" => Slot::Weapon2,
        "Offhand2" => Slot::Offhand2,
        "Helm1" => Slot::Helmet,
        "BodyArmour1" => Slot::Body,
        "Gloves1" => Slot::Gloves,
        "Boots1" => Slot::Boots,
        "Belt1" => Slot::Belt,
        "Amulet1" => Slot::Amulet,
        "Ring1" => Slot::RingLeft,
        "Ring2" => Slot::RingRight,
        _ => return None,
    })
}

/// Emplacement d'un unique recommandé ; `None` si l'emplacement est inconnu.
fn wanted_slot(inventory_id: &str) -> Option<Option<Slot>> {
    Some(match inventory_id {
        id if id.starts_with("Flask") => Some(Slot::LifeFlask),
        id if id.starts_with("Charm") => Some(Slot::Charm1),
        id => Some(slot_of(id)?),
    })
}

fn parse_targets(text: &str) -> (String, Vec<Target>) {
    let mut lines = text.lines();
    let base = lines.next().unwrap_or_default().trim().to_string();
    let targets = lines
        .filter_map(|l| TARGET_LINE.captures(l.trim()).map(|c| c[1].trim().to_string()))
        .filter_map(|line| {
            let (template, value) = crate::trade::template(&line);
            Some(Target { template, value: value?, line })
        })
        .collect();
    (base, targets)
}

static STEP_LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Act \d+|Endgame Step \d+)").unwrap());
/// Étape par tranche de niveaux (Mobalytics) : « 1 - 21 - [0.5.5] Ulf's EARTHSHATTER Leag »,
/// « 22 - End of Campaign - … ».
static LEVEL_STEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(\d+)\s*-\s*(\d+|end of campaign)\s*-\s*(.+)$").unwrap());
static LEVEL_LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Niv\. (\d+)(?:-(\d+)|\+)$").unwrap());

#[derive(Deserialize)]
pub struct RawBuild {
    pub name: String,
    #[serde(default)]
    skills: Vec<RawSkill>,
    #[serde(default)]
    passives: Vec<RawPassive>,
    #[serde(default)]
    inventory_slots: Vec<RawSlot>,
}
#[derive(Deserialize)]
struct RawSlot {
    inventory_id: String,
    #[serde(default)]
    additional_text: String,
    #[serde(default)]
    unique_name: String,
    /// Position dans la rangée (fioles : 0 vie, 1 mana ; charmes : 0 à 2).
    #[serde(default)]
    slot_x: usize,
}
#[derive(Deserialize)]
struct RawSkill {
    id: String,
    #[serde(default = "full_range")]
    level_interval: [u32; 2],
    #[serde(default)]
    support_skills: Vec<RawSkill>,
}
fn full_range() -> [u32; 2] {
    [1, 100]
}

#[derive(Deserialize)]
struct RawPassive {
    id: String,
    #[serde(default)]
    weapon_set: Option<u8>,
}

/// Étiquette d'étape et nom du build, depuis le champ `name` d'un fichier .build : par acte
/// ("Act 1 {1} - Cold Caster Chronomancer - L" → "Act 1"), endgame ("Endgame Step 1 - …"),
/// par tranche de niveaux ("1 - 21 - Ulf's EARTHSHATTER" → "Niv. 1-21", "22 - End of
/// Campaign - …" → "Niv. 22+") ou variante d'endgame nommée ("BUDGET - …" → "BUDGET").
pub fn split_name(name: &str) -> (Option<String>, String) {
    if let Some(c) = LEVEL_STEP.captures(name) {
        let label = if c[2].chars().all(|ch| ch.is_ascii_digit()) { format!("Niv. {}-{}", &c[1], &c[2]) } else { format!("Niv. {}+", &c[1]) };
        return (Some(label), c[3].trim().to_string());
    }
    if let Some(c) = STEP_LABEL.captures(name) {
        let rest = name.split(" - ").nth(1).unwrap_or(name).trim().to_string();
        return (Some(c[1].to_string()), rest);
    }
    match name.split_once(" - ") {
        // « Nonsense Minmax Endgame Step 4 {9} » (Mobalytics) : le numéro d'ordre n'est pas
        // dans l'étiquette.
        Some((label, rest)) if !label.trim().is_empty() => (Some(label.split(" {").next().unwrap_or(label).trim().to_string()), rest.trim().to_string()),
        _ => (None, name.trim().to_string()),
    }
}

/// Tranche de niveaux d'une étape « Niv. 1-21 » (« Niv. 22+ » : jusqu'à la fin de la campagne).
pub fn level_range(label: &str) -> Option<(u32, u32)> {
    let c = LEVEL_LABEL.captures(label)?;
    let hi = match c.get(2) {
        Some(h) => h.as_str().parse().ok()?,
        None => u32::MAX,
    };
    Some((c[1].parse().ok()?, hi))
}

/// Étape d'endgame : « Endgame Step N » ou variante nommée (ni acte, ni tranche de niveaux).
pub fn is_endgame(label: &str) -> bool {
    level_range(label).is_none() && !label.starts_with("Act ")
}

/// Étapes d'endgame, au choix du joueur une fois la campagne finie.
pub fn endgame_steps(steps: &[Step]) -> Vec<&Step> {
    steps.iter().filter(|s| is_endgame(&s.label)).collect()
}

/// Passifs propres à un set d'armes. Un même passif peut être listé plusieurs fois (commun et
/// set I, ou set I et set II) : il n'est propre à un set que s'il n'est jamais commun et
/// toujours dans ce même set.
fn weapon_set_passives(passives: &[RawPassive]) -> HashMap<String, u8> {
    let mut sets: HashMap<&str, Vec<Option<u8>>> = HashMap::new();
    for p in passives {
        sets.entry(p.id.as_str()).or_default().push(p.weapon_set);
    }
    sets.into_iter()
        .filter_map(|(id, s)| {
            let first = s[0]?;
            s.iter().all(|w| *w == Some(first)).then(|| (id.to_string(), first))
        })
        .collect()
}

/// Étapes de build lues dans des fichiers .build, dans l'ordre donné.
pub fn load_steps(files: &[PathBuf], catalog: &Catalog) -> Vec<Step> {
    let sets = files.first().and_then(|f| f.parent()).map(crate::builds::load_skill_sets).unwrap_or_default();
    files
        .iter()
        .filter_map(|p| serde_json::from_str::<RawBuild>(&std::fs::read_to_string(p).ok()?).ok())
        .filter_map(|b| {
            let label = split_name(&b.name).0?;
            let step_sets = sets.iter().find(|s| s.name == label);
            let skills = b
                .skills
                .iter()
                .map(|s| {
                    let name = catalog.gem(&s.id);
                    Skill {
                        // Sets connus pour l'étape (Mobalytics) : un sort sans set s'utilise avec
                        // les deux (0).
                        weapon_set: step_sets.map(|s| s.sets.get(&name).copied().unwrap_or(0)),
                        name,
                        from_level: s.level_interval[0],
                        supports: s.support_skills.iter().map(|u| (catalog.gem(&u.id), u.level_interval[0])).collect(),
                    }
                })
                .collect();
            let targets = b
                .inventory_slots
                .iter()
                .filter_map(|s| {
                    let slot = slot_of(&s.inventory_id)?;
                    let (base, targets) = parse_targets(&s.additional_text);
                    Some((slot, base, targets))
                })
                .collect();
            // Fioles et charmes : tous rangés sous Flask1 / Charm1 par le planificateur. Une fiole
            // de mana se reconnaît à son type de base, sinon à sa place dans la rangée.
            let side_targets = b
                .inventory_slots
                .iter()
                .filter(|s| s.unique_name.is_empty() && !s.additional_text.is_empty())
                .filter_map(|s| {
                    let (base, targets) = parse_targets(&s.additional_text);
                    let slot = match s.inventory_id.as_str() {
                        id if id.starts_with("Flask") && base.contains("Mana Flask") => Slot::ManaFlask,
                        id if id.starts_with("Flask") && base.contains("Life Flask") => Slot::LifeFlask,
                        id if id.starts_with("Flask") => [Slot::LifeFlask, Slot::ManaFlask].get(s.slot_x).copied()?,
                        id if id.starts_with("Charm") => crate::gear::CHARMS.get(s.slot_x).copied()?,
                        _ => return None,
                    };
                    Some((slot, base, targets))
                })
                .collect();
            let uniques = b
                .inventory_slots
                .iter()
                .filter(|s| !s.unique_name.is_empty())
                .filter_map(|s| Some(Wanted { name: s.unique_name.clone(), slot: wanted_slot(&s.inventory_id)?, position: s.slot_x }))
                .collect();
            let passive_sets = weapon_set_passives(&b.passives);
            Some(Step { label, skills, passives: b.passives.into_iter().map(|p| p.id).collect(), targets, uniques, passive_sets, side_targets })
        })
        .collect()
}

/// Pendant la campagne : l'étape de l'acte, ou la tranche de niveaux qui contient le niveau.
/// En endgame (acte 6) : l'étape d'endgame choisie (`endgame_step`, à partir de 1), sinon la
/// dernière tranche de niveaux.
pub fn step_for(steps: &[Step], act: u32, endgame_step: u32, level: u32) -> Option<&Step> {
    let ranged: Vec<(&Step, (u32, u32))> = steps.iter().filter_map(|s| Some((s, level_range(&s.label)?))).collect();
    if act > 5 {
        let endgame = endgame_steps(steps);
        if !endgame.is_empty() {
            return Some(endgame[(endgame_step.max(1) as usize - 1).min(endgame.len() - 1)]);
        }
        return ranged.last().map(|(s, _)| *s);
    }
    if let Some(s) = steps.iter().find(|s| s.label == format!("Act {act}")) {
        return Some(s);
    }
    ranged
        .iter()
        .find(|(_, (lo, hi))| (*lo..=*hi).contains(&level))
        .or_else(|| ranged.iter().filter(|(_, (lo, _))| *lo <= level).last())
        .or(ranged.first())
        .map(|(s, _)| *s)
}

/// Gemmes et supports qui deviennent pertinents en passant de `old` à `new`.
pub fn new_at_level(step: &Step, old: u32, new: u32) -> Vec<String> {
    let mut out = Vec::new();
    for s in &step.skills {
        if old < s.from_level && s.from_level <= new {
            out.push(format!("sort {}", s.describe()));
        }
        for (name, lvl) in &s.supports {
            if s.from_level <= old && old < *lvl && *lvl <= new {
                out.push(format!("support {name} sur {}", s.name));
            }
        }
    }
    out
}

pub fn ascendancy_notables(step: &Step, catalog: &Catalog) -> Vec<String> {
    step.passives
        .iter()
        .filter_map(|id| catalog.passives.get(id))
        .filter(|p| p.ascendancy && p.notable)
        .map(|p| p.name.clone())
        .collect()
}

/// Ce qui change d'une étape à l'autre.
pub fn diff_steps(old: Option<&Step>, new: &Step, catalog: &Catalog) -> Vec<String> {
    let mut lines = Vec::new();
    let old_names: HashSet<&str> = old.map(|o| o.skills.iter().map(|s| s.name.as_str()).collect()).unwrap_or_default();
    let added: Vec<String> = new.skills.iter().filter(|s| !old_names.contains(s.name.as_str())).map(Skill::describe).collect();
    if !added.is_empty() {
        lines.push(format!("Nouveaux sorts : {}", added.join(" ; ")));
    }
    if let Some(old) = old {
        let new_names: HashSet<&str> = new.skills.iter().map(|s| s.name.as_str()).collect();
        let removed: Vec<&str> = old.skills.iter().map(|s| s.name.as_str()).filter(|n| !new_names.contains(n)).collect();
        if !removed.is_empty() {
            lines.push(format!("Sorts abandonnés : {}", removed.join(", ")));
        }
        for s in &new.skills {
            if let Some(prev) = old.skills.iter().find(|o| o.name == s.name) {
                let names = |sk: &Skill| sk.supports.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
                if names(prev) != names(s) {
                    lines.push(format!("Supports de {} : {}", s.name, names(s).join(", ")));
                }
            }
        }
    }
    let old_ids: HashSet<&str> = old.map(|o| o.passives.iter().map(String::as_str).collect()).unwrap_or_default();
    let fresh: Vec<&Passive> = new.passives.iter().filter(|id| !old_ids.contains(id.as_str())).filter_map(|id| catalog.passives.get(id)).collect();
    let notables: Vec<&str> = fresh.iter().filter(|p| p.notable && !p.ascendancy).map(|p| p.name.as_str()).collect();
    if !notables.is_empty() {
        lines.push(format!("Notables à prendre : {}", notables.join(", ")));
    }
    let asc: Vec<&str> = fresh.iter().filter(|p| p.notable && p.ascendancy).map(|p| p.name.as_str()).collect();
    if !asc.is_empty() {
        lines.push(format!("Ascendance : {}", asc.join(", ")));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(label: &str, skills: &[(&str, u32)]) -> Step {
        Step {
            label: label.into(),
            skills: skills.iter().map(|(n, l)| Skill { name: n.to_string(), from_level: *l, supports: vec![], weapon_set: None }).collect(),
            passives: vec![],
            targets: vec![],
            uniques: vec![],
            passive_sets: HashMap::new(), side_targets: vec![],
        }
    }

    #[test]
    fn passives_listed_twice_are_common() {
        let p = |id: &str, w: Option<u8>| RawPassive { id: id.into(), weapon_set: w };
        let sets = weapon_set_passives(&[p("a", None), p("a", Some(1)), p("b", Some(1)), p("b", Some(2)), p("c", Some(2)), p("c", Some(2)), p("d", Some(1))]);
        assert_eq!(sets.len(), 2);
        assert_eq!((sets["c"], sets["d"]), (2, 1));
    }

    #[test]
    fn unique_text() {
        let lua = "return {\n[[\nOlroth's Resolve\nUltimate Life Flask\nSource: Drops\nVersion: Pre 0.4.0\nVersion: Current\n{version:1}Instant Recovery\n(100-150)% increased Charges per use\n{version:2}Regenerate 3% of Ward\n]],[[\nRite of Passage\nGolden Charm\nLeague: Dawn\nVariant: Owl\nVariant: Bear\nImplicits: 1\nUsed when you kill a Rare enemy\n{variant:2}{tags:x}Possessed by Bear\n]],\n}";
        let u = parse_uniques(lua);
        assert_eq!(u[0].0, "Olroth's Resolve");
        assert_eq!((u[0].1.base.as_str(), u[0].1.mods.clone()), ("Ultimate Life Flask", vec!["(100-150)% increased Charges per use".to_string(), "Regenerate 3% of Ward".to_string()]));
        assert_eq!(u[1].1.implicits, vec!["Used when you kill a Rare enemy"]);
        assert_eq!(u[1].1.mods, vec!["Possessed by Bear"]);
        let choice = "[[\nRite\nGolden Charm\nVariant: Owl\nVariant: Bear\n{variant:1}Possessed by Spirit Of The Owl for 10 seconds\n{variant:2}Possessed by Spirit Of The Bear for 10 seconds\n]]";
        assert_eq!(parse_uniques(choice)[0].1.mods, vec!["Possessed by a random Spirit for 10 seconds"]);
        let other = "[[\nX\nRuby Ring\nVariant: Fire\nVariant: Cold\n{variant:1}+10% to Fire Resistance\n{variant:2}+10% to Cold Resistance\n]]";
        assert_eq!(parse_uniques(other)[0].1.mods, vec!["+10% to [au choix] Resistance"]);
    }

    #[test]
    fn charm_buffs() {
        let lua = "itemBases[\"Golden Charm\"] = {\n\ttype = \"Charm\",\n\tcharm = { duration = 1, chargesUsed = 80, chargesMax = 80, buff = { \"15% increased Rarity of Items found\" }, },\n}\nitemBases[\"Ultimate Mana Flask\"] = {\n\tflask = { mana = 310, },\n}\n";
        let b = parse_charm_buffs(lua);
        assert_eq!(b.get("Golden Charm").unwrap(), &vec!["15% increased Rarity of Items found".to_string()]);
        assert!(!b.contains_key("Ultimate Mana Flask"));
    }

    #[test]
    fn targets_parsing() {
        let (base, t) = parse_targets("Gelid Staff\n1. 54% increased Spell Damage\n2. +2 to Level of all Spell Skills\n\nRubDubDucky is a cool guy");
        assert_eq!(base, "Gelid Staff");
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].template.as_str(), t[0].value), ("#% increased Spell Damage", 54.0));
        assert_eq!((t[1].template.as_str(), t[1].value), ("# to Level of all Spell Skills", 2.0));
        assert_eq!(split_name("Act 1 {1} - Cold Caster Chronomancer - L"), (Some("Act 1".into()), "Cold Caster Chronomancer".into()));
        assert_eq!(split_name("1 - 21 - [0.5.5] Ulf's EARTHSHATTER Leag"), (Some("Niv. 1-21".into()), "[0.5.5] Ulf's EARTHSHATTER Leag".into()));
        assert_eq!(level_range("Niv. 22-45"), Some((22, 45)));
        assert_eq!(split_name("22 - End of Campaign - [0.5.5] Ulf's EAR"), (Some("Niv. 22+".into()), "[0.5.5] Ulf's EAR".into()));
        assert_eq!(split_name("STR FACEBREAKER - [0.5.5] Ulf's EARTHSHA").0.as_deref(), Some("STR FACEBREAKER"));
        assert_eq!(level_range("Niv. 22+"), Some((22, u32::MAX)));
    }

    #[test]
    fn steps_and_levels() {
        let steps = vec![step("Act 1", &[("Contagion", 1), ("Bonestorm", 15)]), step("Act 2", &[("Ice Nova", 1)])];
        assert_eq!(step_for(&steps, 1, 1, 1).unwrap().label, "Act 1");
        assert!(step_for(&steps, 6, 1, 70).is_none());
        // Build découpé par niveaux : la tranche du niveau, la dernière au-delà.
        let by_level = vec![step("Niv. 1-21", &[]), step("Niv. 22-45", &[])];
        assert_eq!(step_for(&by_level, 2, 1, 30).unwrap().label, "Niv. 22-45");
        assert_eq!(step_for(&by_level, 6, 1, 80).unwrap().label, "Niv. 22-45");
        // Campagne par niveaux puis variantes d'endgame au choix.
        let ulf = vec![step("Niv. 1-21", &[]), step("Niv. 22+", &[]), step("BUDGET", &[]), step("GIGA Olrovasara", &[])];
        assert_eq!(step_for(&ulf, 4, 1, 60).unwrap().label, "Niv. 22+");
        assert_eq!(step_for(&ulf, 6, 2, 80).unwrap().label, "GIGA Olrovasara");
        assert_eq!(step_for(&ulf, 6, 9, 80).unwrap().label, "GIGA Olrovasara");
        assert_eq!(new_at_level(&steps[0], 14, 15), vec!["sort Bonestorm"]);
        let diff = diff_steps(Some(&steps[0]), &steps[1], &Catalog::default());
        assert_eq!(diff, vec!["Nouveaux sorts : Ice Nova", "Sorts abandonnés : Contagion, Bonestorm"]);
    }

    #[test]
    fn gem_name_fallback() {
        assert_eq!(Catalog::default().gem("Metadata/Items/Gems/SkillGemIceNova"), "Ice Nova");
    }
}
