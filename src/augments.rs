//! Augments (runes, soul cores, idoles) : catalogue RePoE, reconnaissance de ceux posés dans
//! un objet et suggestions pour ses châsses vides.
//!
//! Le texte copié en jeu donne le nombre de châsses (« Sockets: S S ») et l'effet des
//! augments posés (lignes « (rune) »), pas leur nom : on le retrouve en cherchant quels
//! augments du catalogue donnent exactement ces effets sur ce type d'objet. Deux augments au
//! même effet sont fusionnés par le jeu (deux Desert Runes : « 16% … ») : on décompose.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::art::USER_AGENT;
use crate::item::{Item, ModKind};
use crate::trade::template;

const AUGMENTS: &str = "https://repoe-fork.github.io/poe2/augments.min.json";
const BASES: &str = "https://repoe-fork.github.io/poe2/base_items.min.json";
const MODS: &str = "https://repoe-fork.github.io/poe2/mods.min.json";
const CACHE: &str = "augments_v5.json";
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Augment {
    pub name: String,
    /// Dernière partie de l'identifiant du jeu, en minuscules (« Metadata/Items/SoulCores/
    /// RuneSpecial9 » -> « runespecial9 ») : Mobalytics désigne les runes ainsi.
    #[serde(default)]
    pub key: String,
    /// "Rune", "SoulCore", "Idol"…
    pub kind: String,
    pub level: u32,
    pub effects: Vec<Effect>,
}

/// Catalogue : augments, et ce qu'apportent les types d'objets eux-mêmes (défenses de base :
/// armure, évasion, bouclier d'énergie ; implicites), téléchargés en même temps.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub augments: Vec<Augment>,
    pub defences: HashMap<String, [f64; 3]>,
    pub implicits: HashMap<String, Vec<String>>,
    /// Classe de chaque type d'objet équipable (« Staff », « Shield »…) et élément de sa
    /// compétence implicite pour les bâtons et baguettes (un bâton de foudre n'a pas de mods de
    /// sorts de froid).
    #[serde(default)]
    pub classes: HashMap<String, (String, Option<String>)>,
}

/// Ce qu'apporte un type d'objet par lui-même : de quoi juger s'il convient à un build.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BaseMeta {
    pub class: String,
    pub element: Option<String>,
    /// Armure, évasion, bouclier d'énergie.
    pub defences: [f64; 3],
    pub implicits: Vec<String>,
}

/// Effet d'un augment sur une catégorie d'objets ("Armour", "Wand or Staff"…).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub category: String,
    /// Modèle de stat et valeur, une entrée par ligne.
    pub stats: Vec<(String, f64)>,
    /// Lignes lisibles, balisage retiré.
    pub text: Vec<String>,
}

/// Catégories d'augments qui s'appliquent à une classe d'objets.
fn categories(item_class: &str) -> &'static [&'static str] {
    const MARTIAL: &[&str] = &["Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"];
    match item_class {
        "Helmets" => &["Helmet", "Armour", "All"],
        "Body Armours" => &["Body Armour", "Armour", "All"],
        "Gloves" => &["Gloves", "Armour", "All"],
        "Boots" => &["Boots", "Armour", "All"],
        "Shields" => &["Shield", "Shield or Buckler", "Armour", "All"],
        "Bucklers" => &["Buckler", "Shield or Buckler", "Armour", "All"],
        "Foci" => &["Focus", "Armour", "All"],
        "Staves" => &["Staff", "Wand or Staff", "Caster Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Wands" => &["Wand", "Wand or Staff", "Caster Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Sceptres" => &["Sceptre", "Caster Weapon", "Martial Or Caster Weapon", "All"],
        "One Hand Maces" => &["One Hand Mace", "One Hand Mace or Quarterstaff", "Maces or Talisman", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Two Hand Maces" => &["Two Hand Mace", "Maces or Talisman", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Quarterstaves" => &["Quarterstaff", "One Hand Mace or Quarterstaff", "Quarterstaff or Spear", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Spears" => &["Spear", "Quarterstaff or Spear", "Crossbow Bow or Spear", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Bows" => &["Bow", "Crossbow Bow or Spear", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Crossbows" => &["Crossbow", "Crossbow Bow or Spear", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "Talismans" => &["Talisman", "Maces or Talisman", "Martial Weapon", "Martial Or Caster Weapon", "Martial Weapon Wand or Staff", "All"],
        "One Hand Swords" | "Two Hand Swords" | "One Hand Axes" | "Two Hand Axes" | "Daggers" | "Claws" | "Flails" => MARTIAL,
        _ => &[],
    }
}

impl Catalog {
    /// Catalogue tel qu'en cache, sans téléchargement (outils en ligne de commande).
    pub fn cached(cache: &Path) -> Option<Catalog> {
        serde_json::from_str(&std::fs::read_to_string(cache.join(CACHE)).ok()?).ok()
    }

    pub fn base(&self, name: &str) -> Option<BaseMeta> {
        let (class, element) = self.classes.get(name)?.clone();
        Some(BaseMeta {
            class,
            element,
            defences: self.defences.get(name).copied().unwrap_or_default(),
            implicits: self.implicits.get(name).cloned().unwrap_or_default(),
        })
    }

    /// Nom d'un augment d'après son identifiant Mobalytics (« soulcore-runespecial9 »), ou
    /// l'identifiant tel quel s'il est inconnu (c'est déjà un nom : Kami-labs).
    pub fn rune_name(&self, id: &str) -> String {
        let key = id.strip_prefix("soulcore-").unwrap_or(id);
        self.augments.iter().find(|a| a.key == key).map(|a| a.name.clone()).unwrap_or_else(|| id.to_string())
    }
}

impl Augment {
    /// Effet de cet augment sur une classe d'objets (catégorie la plus précise d'abord).
    pub fn effect_for(&self, item_class: &str) -> Option<&Effect> {
        categories(item_class).iter().find_map(|c| self.effects.iter().find(|e| e.category == *c))
    }
}

/// Retire le balisage RePoE : « [Resistances|Cold Resistance] » -> « Cold Resistance ».
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find(']') else { break };
        let inner = &rest[start + 1..start + end];
        out.push_str(inner.rsplit('|').next().unwrap_or(inner));
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

fn build_catalog(augments: &serde_json::Value, bases: &serde_json::Value) -> Vec<Augment> {
    let mut out = Vec::new();
    for (id, a) in augments.as_object().into_iter().flatten() {
        let Some(name) = bases[id]["name"].as_str().filter(|n| !n.is_empty()) else { continue };
        let effects = a["categories"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(category, c)| {
                let text: Vec<String> = c["stat_text"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str())
                    .flat_map(|t| t.lines().map(clean).collect::<Vec<_>>())
                    .collect();
                let stats = text.iter().filter_map(|l| match template(l) {
                    (t, Some(v)) => Some((t, v)),
                    _ => None,
                });
                Effect { category: category.clone(), stats: stats.collect(), text }
            })
            .collect();
        out.push(Augment {
            name: name.to_string(),
            key: id.rsplit('/').next().unwrap_or(id).to_lowercase(),
            kind: a["type_id"].as_str().unwrap_or_default().to_string(),
            level: a["required_level"].as_u64().unwrap_or(0) as u32,
            effects,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Défenses de base (armure, évasion, bouclier d'énergie) de chaque type d'objet qui en a.
fn base_defences(bases: &serde_json::Value) -> HashMap<String, [f64; 3]> {
    let mut out = HashMap::new();
    for b in bases.as_object().into_iter().flatten().map(|(_, b)| b) {
        let Some(name) = b["name"].as_str().filter(|n| !n.is_empty()) else { continue };
        let p = &b["properties"];
        let get = |k: &str| p[k]["max"].as_f64().unwrap_or(0.0);
        let d = [get("armour"), get("evasion"), get("energy_shield")];
        if d.iter().any(|v| *v > 0.0) {
            out.entry(name.to_string()).or_insert(d);
        }
    }
    out
}

/// Classe et élément de chaque type d'objet équipable.
fn base_classes(bases: &serde_json::Value) -> HashMap<String, (String, Option<String>)> {
    let mut out = HashMap::new();
    for b in bases.as_object().into_iter().flatten().map(|(_, b)| b) {
        let Some(name) = b["name"].as_str().filter(|n| !n.is_empty()) else { continue };
        let Some(class) = b["item_class"].as_str() else { continue };
        if b["domain"].as_str() != Some("item") {
            continue;
        }
        let tags: Vec<&str> = b["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).collect();
        let element = tags.iter().find_map(|t| t.strip_suffix("_implicit_skill")).map(str::to_string);
        out.entry(name.to_string()).or_insert((class.to_string(), element));
    }
    out
}

/// Texte des implicites de chaque type d'objet qui en a.
fn base_implicits(bases: &serde_json::Value, mods: &serde_json::Value) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    for b in bases.as_object().into_iter().flatten().map(|(_, b)| b) {
        let Some(name) = b["name"].as_str().filter(|n| !n.is_empty()) else { continue };
        let lines: Vec<String> = b["implicits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| mods[id.as_str()?]["text"].as_str())
            .flat_map(|t| t.lines().map(clean).collect::<Vec<_>>())
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() {
            out.entry(name.to_string()).or_insert(lines);
        }
    }
    out
}

fn download() -> Result<Catalog, Box<dyn std::error::Error>> {
    let get = |url: &str| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(ureq::get(url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(60)).call()?.into_json()?)
    };
    let bases = get(BASES)?;
    let implicits = base_implicits(&bases, &get(MODS)?);
    Ok(Catalog { augments: build_catalog(&get(AUGMENTS)?, &bases), defences: base_defences(&bases), implicits, classes: base_classes(&bases) })
}

fn fresh(path: &Path) -> bool {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age < MAX_AGE)
}

/// Catalogue partagé avec l'interface, chargé (ou téléchargé) en arrière-plan.
#[derive(Clone)]
pub struct Augments {
    data: Arc<Mutex<Option<Arc<Catalog>>>>,
    cache: PathBuf,
}

impl Augments {
    pub fn start(cache: PathBuf) -> Augments {
        let shared = Augments { data: Arc::default(), cache };
        shared.load(false);
        shared
    }

    /// Retélécharge le catalogue sans attendre qu'il soit périmé.
    pub fn refresh(&self) {
        self.load(true);
    }

    fn load(&self, force: bool) {
        let (out, cache) = (self.data.clone(), self.cache.clone());
        std::thread::spawn(move || {
            let path = cache.join(CACHE);
            let cached: Option<Catalog> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
            if let Some(c) = &cached {
                if out.lock().unwrap().is_none() {
                    *out.lock().unwrap() = Some(Arc::new(c.clone()));
                }
            }
            if force || cached.is_none() || !fresh(&path) {
                match download() {
                    Ok(c) => {
                        let _ = std::fs::create_dir_all(&cache);
                        let _ = std::fs::write(&path, serde_json::to_string(&c).unwrap_or_default());
                        *out.lock().unwrap() = Some(Arc::new(c));
                    }
                    Err(e) => eprintln!("catalogue indisponible : {e}"),
                }
            }
        });
    }

    pub fn get(&self) -> Option<Arc<Catalog>> {
        self.data.lock().unwrap().clone()
    }
}

/// Châsses d'un objet : augments reconnus, châsses vides, effets non reconnus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Socketed {
    pub sockets: usize,
    pub augments: Vec<String>,
    pub unknown: Vec<String>,
}

impl Socketed {
    pub fn empty(&self) -> usize {
        self.sockets.saturating_sub(self.augments.len() + self.unknown.len())
    }
}

/// Augments posés dans `it`, retrouvés d'après les effets « (rune) » de son texte.
pub fn identify(it: &Item, catalog: &[Augment]) -> Socketed {
    let lines: Vec<&String> = it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k == ModKind::Rune).map(|(m, _)| m).collect();
    let mut remaining: BTreeMap<String, f64> = BTreeMap::new();
    let mut text_of: HashMap<String, String> = HashMap::new();
    for l in &lines {
        if let (t, Some(v)) = template(l) {
            *remaining.entry(t.clone()).or_insert(0.0) += v;
            text_of.insert(t, (*l).clone());
        }
    }
    let candidates: Vec<(&Augment, &Effect)> = catalog
        .iter()
        .filter_map(|a| Some((a, a.effect_for(&it.item_class)?)))
        .filter(|(_, e)| !e.stats.is_empty() && e.stats.iter().all(|(t, _)| remaining.contains_key(t)))
        .collect();
    // Le moins d'augments possible qui expliquent exactement les effets, sans dépasser les châsses.
    let max = it.sockets.max(lines.len());
    let mut found = None;
    for depth in 1..=max {
        let mut picked = Vec::new();
        if solve(&candidates, 0, depth, &mut remaining.clone(), &mut picked) {
            found = Some(picked);
            break;
        }
    }
    let augments: Vec<String> = found.as_ref().map(|p| p.iter().map(|i| candidates[*i].0.name.clone()).collect()).unwrap_or_default();
    let unknown = if found.is_some() || remaining.is_empty() { vec![] } else { remaining.keys().filter_map(|t| text_of.get(t).cloned()).collect() };
    Socketed { sockets: it.sockets.max(augments.len() + unknown.len()), augments, unknown }
}

/// Retire des augments (indices croissants, répétitions permises) jusqu'à tout expliquer.
fn solve(candidates: &[(&Augment, &Effect)], from: usize, left: usize, remaining: &mut BTreeMap<String, f64>, picked: &mut Vec<usize>) -> bool {
    if remaining.values().all(|v| v.abs() < 0.01) {
        return true;
    }
    if left == 0 {
        return false;
    }
    for i in from..candidates.len() {
        let e = candidates[i].1;
        if !e.stats.iter().all(|(t, v)| remaining.get(t).is_some_and(|r| *r + 0.01 >= *v)) {
            continue;
        }
        for (t, v) in &e.stats {
            *remaining.get_mut(t).unwrap() -= v;
        }
        picked.push(i);
        if solve(candidates, i, left - 1, remaining, picked) {
            return true;
        }
        picked.pop();
        for (t, v) in &e.stats {
            *remaining.get_mut(t).unwrap() += v;
        }
    }
    false
}

/// Meilleurs augments pour une châsse vide de `it` selon `rate` (intérêt pour le build de
/// stats données), accessibles au niveau `level`. Un seul palier par effet : le meilleur.
pub fn suggestions<'a>(it: &Item, catalog: &'a [Augment], level: u32, rate: impl Fn(&BTreeMap<String, f64>) -> f64) -> Vec<(&'a Augment, &'a Effect)> {
    let mut scored: Vec<(f64, &Augment, &Effect)> = catalog
        .iter()
        .filter(|a| matches!(a.kind.as_str(), "Rune" | "SoulCore" | "Idol") && a.level <= level)
        .filter_map(|a| {
            let e = a.effect_for(&it.item_class)?;
            let values: BTreeMap<String, f64> = e.stats.iter().cloned().collect();
            let score = rate(&values);
            (score > 0.0).then_some((score, a, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out: Vec<(&Augment, &Effect)> = Vec::new();
    for (_, a, e) in scored {
        let family: Vec<&String> = e.stats.iter().map(|(t, _)| t).collect();
        if out.iter().any(|(_, o)| o.stats.iter().map(|(t, _)| t).collect::<Vec<_>>() == family) {
            continue;
        }
        out.push((a, e));
        if out.len() == 3 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item;

    fn rune(name: &str, staff: &str, armour: &str) -> Augment {
        let effect = |category: &str, line: &str| {
            let (t, v) = template(line);
            Effect { category: category.into(), stats: vec![(t, v.unwrap())], text: vec![line.into()] }
        };
        Augment { name: name.into(), key: String::new(), kind: "Rune".into(), level: 1, effects: vec![effect("Wand or Staff", staff), effect("Armour", armour)] }
    }

    fn catalog() -> Vec<Augment> {
        vec![
            rune("Desert Rune", "Gain 8% of Damage as Extra Fire Damage", "+14% to Fire Resistance"),
            rune("Lesser Iron Rune", "20% increased Spell Damage", "14% increased Armour, Evasion and Energy Shield"),
            rune("Iron Rune", "25% increased Spell Damage", "16% increased Armour, Evasion and Energy Shield"),
            rune("Lesser Glacial Rune", "Gain 6% of Damage as Extra Cold Damage", "+10% to Cold Resistance"),
        ]
    }

    const STAFF: &str = "Item Class: Staves\nRarity: Rare\nGale Pile\nChiming Staff\n--------\nSockets: S S S \n--------\nItem Level: 40\n--------\nGain 16% of Damage as Extra Fire Damage (rune)\n45% increased Spell Damage (rune)\n--------\n+3 to Level of all Cold Spell Skills";

    #[test]
    fn merged_runes_are_split_and_empty_sockets_counted() {
        // 3 châsses, mais 16% de feu = 2 Desert Runes et 45% = Lesser Iron + Iron : impossible en 3,
        // donc effets non reconnus ; avec 4 châsses (corruption), tout s'explique.
        let it = item::parse(STAFF).unwrap();
        assert_eq!(it.sockets, 3);
        let s = identify(&it, &catalog());
        assert!(s.augments.is_empty() && s.unknown.len() == 2);
        let corrupted = item::parse(&STAFF.replace("S S S", "S S S S")).unwrap();
        let s = identify(&corrupted, &catalog());
        assert_eq!(s.augments, ["Desert Rune", "Desert Rune", "Lesser Iron Rune", "Iron Rune"]);
        assert_eq!(s.empty(), 0);
        let one = item::parse(&STAFF.replace("Gain 16% of Damage as Extra Fire Damage (rune)\n45%", "20%")).unwrap();
        let s = identify(&one, &catalog());
        assert_eq!((s.augments.clone(), s.empty()), (vec!["Lesser Iron Rune".to_string()], 2));
    }

    #[test]
    fn suggestions_follow_the_build_and_keep_the_best_tier() {
        let boots = item::parse("Item Class: Boots\nRarity: Rare\nX\nSandals\n--------\nSockets: S \n--------\nItem Level: 40").unwrap();
        let wants_res = |v: &BTreeMap<String, f64>| v.keys().filter(|t| t.contains("Resistance")).count() as f64;
        let cat = catalog();
        let s = suggestions(&boots, &cat, 60, wants_res);
        let names: Vec<&str> = s.iter().map(|(a, _)| a.name.as_str()).collect();
        assert_eq!(names, ["Desert Rune", "Lesser Glacial Rune"]);
    }

    /// Réseau : catalogue réel, runes de l'équipement suivi (données locales).
    #[test]
    #[ignore]
    fn real_catalog_on_tracked_gear() {
        let cat = download().unwrap();
        eprintln!("{} augments, {} bases défensives", cat.augments.len(), cat.defences.len());
        assert_eq!(cat.defences["Ancestral Tiara"], [0.0, 0.0, 109.0]);
        eprintln!("Prismatic Ring : {:?}", cat.implicits.get("Prismatic Ring"));
        assert!(cat.implicits["Prismatic Ring"][0].contains("all Elemental Resistances"));
        let cat = cat.augments;
        let dir = directories::ProjectDirs::from("", "", "poe2-tracker").unwrap();
        let gear = crate::gear::Gear::load(&dir.data_dir().join("characters/Juliana_novachrono/gear.json"));
        for (slot, raw) in &gear.slots {
            let it = item::parse(raw).unwrap();
            if it.sockets > 0 {
                eprintln!("{slot:?} {} : {:?}", it.name, identify(&it, &cat));
            }
        }
    }

    #[test]
    fn markup_is_removed() {
        assert_eq!(clean("+10% to [Resistances|Cold Resistance] and [Spell] Damage"), "+10% to Cold Resistance and Spell Damage");
    }
}
