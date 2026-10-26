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
        let path = cache.join("pob_catalog_v6.json");
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
}

/// Unique recommandé par le build. Le planificateur range toutes les fioles sous `Flask1` :
/// une fiole de mana arrive ici en fiole de vie (voir `uniques::slot`).
#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    pub name: String,
    pub slot: Option<Slot>,
}

impl Step {
    pub fn targets_for(&self, slot: Slot) -> impl Iterator<Item = &Target> {
        self.targets.iter().filter(move |(s, _, _)| *s == slot).flat_map(|(_, _, t)| t)
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
                        weapon_set: step_sets.and_then(|s| s.sets.get(&name).copied()),
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
            let uniques = b
                .inventory_slots
                .iter()
                .filter(|s| !s.unique_name.is_empty())
                .filter_map(|s| Some(Wanted { name: s.unique_name.clone(), slot: wanted_slot(&s.inventory_id)? }))
                .collect();
            let passive_sets = weapon_set_passives(&b.passives);
            Some(Step { label, skills, passives: b.passives.into_iter().map(|p| p.id).collect(), targets, uniques, passive_sets })
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
            passive_sets: HashMap::new(),
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
