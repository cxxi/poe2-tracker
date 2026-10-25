//! Analyse du texte d'un objet copié en jeu (Ctrl+C ou Ctrl+Alt+C).
//!
//! ```text
//! Item Class: Staves
//! Rarity: Unique
//! Dusk Vigil
//! Ashen Staff
//! --------
//! Item Level: 8
//! --------
//! { Unique Modifier — Damage, Caster }
//! 120(80-120)% increased Spell Damage
//! ```

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

const SEPARATOR: &str = "--------";

/// Lignes d'en-tête qui ne sont pas des mods.
const HEADER_KEYS: &[&str] = &[
    "Item Level:", "Requires", "Quality:", "Sockets:", "Stack Size:", "Level:", "Armour:",
    "Evasion Rating:", "Energy Shield:", "Ward:", "Spirit:", "Physical Damage:", "Elemental Damage:",
    "Cold Damage:", "Fire Damage:", "Lightning Damage:", "Chaos Damage:", "Critical Hit Chance:",
    "Attacks per Second:", "Reload Time:", "Block chance:", "Limited to:", "Radius:", "Charm Slots:",
    "Grants Skill:", "Note:",
];
/// Consignes d'utilisation des fioles et charmes.
const INSTRUCTIONS: &[&str] =
    &["Right click to drink", "Used automatically", "Currently has", "Can only hold charges"];

static ANNOTATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\{.*\}$").unwrap());
static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((-?[\d.]+)-(-?[\d.]+)\)").unwrap());
static SUFFIX_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s+—\s+Unscalable Value$|\s+\((implicit|enchant|rune|crafted|desecrated|fractured)\)$")
        .unwrap()
});
static PROP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(Armour|Evasion Rating|Energy Shield|Ward|Spirit): (\d+)").unwrap()
});
/// Dégâts de base d'une arme : « Physical Damage: 11-16 (augmented) », « Elemental Damage:
/// 3-5 (augmented), 2-9 (augmented) » (plusieurs fourchettes possibles sur une ligne).
static WEAPON_DAMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(Physical|Elemental|Fire|Cold|Lightning|Chaos) Damage: (.+)$").unwrap());
static DAMAGE_RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)-(\d+)").unwrap());
static WEAPON_RATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(Attacks per Second|Critical Hit Chance): (\d+(?:[.,]\d+)?)").unwrap());
// "2,50" : le jeu peut utiliser la virgule décimale.
static RECOVERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Recovers (\d+(?:[.,]\d+)?) (Life|Mana) over (\d+(?:[.,]\d+)?) Seconds?").unwrap()
});
static LASTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^Lasts (\d+(?:[.,]\d+)?) Seconds?").unwrap());
static CONSUMES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Consumes (\d+) of (\d+)(?: \(augmented\))? Charges on use").unwrap()
});
static REQ_LEVEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Level (\d+)").unwrap());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    Normal,
    Magic,
    Rare,
    Unique,
    Currency,
    Other,
}

/// Origine d'un mod, pour la recherche au marché (seuls les explicites décrivent l'objet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModKind {
    Explicit,
    Implicit,
    Rune,
    Enchant,
}

fn kind_from_annotation(line: &str) -> ModKind {
    if line.contains("Implicit") {
        ModKind::Implicit
    } else if line.contains("Rune") || line.contains("Augment") {
        ModKind::Rune
    } else if line.contains("Enchant") {
        ModKind::Enchant
    } else {
        ModKind::Explicit
    }
}

/// Format simple (Ctrl+C sans détails) : "+10% to Cold Resistance (implicit)".
fn kind_from_suffix(line: &str) -> Option<ModKind> {
    if line.ends_with("(implicit)") {
        Some(ModKind::Implicit)
    } else if line.ends_with("(rune)") || line.ends_with("(augmented)") {
        Some(ModKind::Rune)
    } else if line.ends_with("(enchant)") {
        Some(ModKind::Enchant)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub item_class: String,
    pub rarity: Rarity,
    /// Nom affiché (première ligne après la rareté).
    pub name: String,
    /// Type de base tel qu'écrit dans le texte. Pour un objet magique, le nom contient
    /// préfixe et suffixe : la vraie base se retrouve avec `art::BaseIndex::base_of`.
    pub base: String,
    pub item_level: u32,
    pub req_level: u32,
    pub stack: u32,
    /// Valeurs numériques d'en-tête : "Energy Shield", "Armour", "recover", "duration"…
    pub props: BTreeMap<String, f64>,
    pub mods: Vec<String>,
    /// Origine de chaque mod de `mods` (même ordre).
    pub mod_kinds: Vec<ModKind>,
    pub granted_skills: Vec<String>,
    pub corrupted: bool,
    pub unidentified: bool,
    pub raw: String,
}

impl Item {
    pub fn title(&self) -> String {
        if self.name == self.base { self.name.clone() } else { format!("{} ({})", self.name, self.base) }
    }
}

pub fn is_item_text(text: &str) -> bool {
    text.starts_with("Item Class:") && text.contains("\nRarity:")
}

fn clean_mod(line: &str) -> String {
    let line = RANGE.replace_all(line, "");
    SUFFIX_TAG.replace_all(&line, "").trim().to_string()
}

fn num(text: &str) -> f64 {
    text.replace(',', ".").parse().unwrap_or(0.0)
}

/// La dernière section des uniques est un texte d'ambiance, pas des mods.
fn is_flavour(section: &[&str]) -> bool {
    if section.iter().any(|l| ANNOTATION.is_match(l.trim())) {
        return false; // section annotée par le jeu : ce sont des mods
    }
    let joined = section.join(" ");
    !joined.chars().any(|c| c.is_ascii_digit())
        && !["increased", "reduced", "to ", "Adds", "Gain", "more", "less", "Grants"]
            .iter()
            .any(|w| joined.contains(w))
}

pub fn parse(text: &str) -> Option<Item> {
    if !is_item_text(text) {
        return None;
    }
    let text = text.replace("\r\n", "\n");
    let text = text.trim();
    let sections: Vec<Vec<&str>> =
        text.split(SEPARATOR).map(|s| s.trim().lines().collect()).collect();
    let head = &sections[0];
    let field = |key: &str| {
        head.iter()
            .find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_string()))
            .unwrap_or_default()
    };
    let names: Vec<&str> = head.iter().skip(2).copied().collect();
    let name = names.first().unwrap_or(&"").to_string();
    let base = names.get(1).map(|s| s.to_string()).unwrap_or_else(|| name.clone());
    let rarity = match field("Rarity:").as_str() {
        "Normal" => Rarity::Normal,
        "Magic" => Rarity::Magic,
        "Rare" => Rarity::Rare,
        "Unique" => Rarity::Unique,
        "Currency" => Rarity::Currency,
        _ => Rarity::Other,
    };
    let mut item = Item {
        item_class: field("Item Class:"),
        rarity,
        name,
        base,
        item_level: 0,
        req_level: 0,
        stack: 1,
        props: BTreeMap::new(),
        mods: Vec::new(),
        mod_kinds: Vec::new(),
        granted_skills: Vec::new(),
        corrupted: false,
        unidentified: false,
        raw: text.to_string(),
    };

    for section in &sections[1..] {
        let mut kind = ModKind::Explicit;
        for line in section {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if ANNOTATION.is_match(line) {
                kind = kind_from_annotation(line);
                continue;
            }
            if let Some(v) = line.strip_prefix("Item Level:") {
                item.item_level = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("Stack Size:") {
                let n: String = v.split('/').next().unwrap_or("").chars().filter(char::is_ascii_digit).collect();
                item.stack = n.parse().unwrap_or(1);
            } else if let Some(v) = line.strip_prefix("Grants Skill:") {
                item.granted_skills.push(v.trim().to_string());
            } else if line == "Corrupted" {
                item.corrupted = true;
            } else if line == "Unidentified" {
                item.unidentified = true;
            } else if let Some(m) = RECOVERS.captures(line) {
                item.props.insert("recover".into(), num(&m[1]));
                item.props.insert("recover_time".into(), num(&m[3]));
            } else if let Some(m) = LASTS.captures(line) {
                item.props.insert("duration".into(), num(&m[1]));
            } else if let Some(m) = CONSUMES.captures(line) {
                item.props.insert("charges_use".into(), num(&m[1]));
                item.props.insert("charges_max".into(), num(&m[2]));
            } else if INSTRUCTIONS.iter().any(|p| line.starts_with(p)) {
                continue;
            } else if line.starts_with("Requires") {
                if let Some(m) = REQ_LEVEL.captures(line) {
                    item.req_level = m[1].parse().unwrap_or(0);
                }
            } else if HEADER_KEYS.iter().any(|k| line.starts_with(k)) || line.ends_with("(augmented)") {
                if let Some(m) = PROP.captures(line) {
                    item.props.insert(m[1].to_string(), num(&m[2]));
                } else if let Some(m) = WEAPON_DAMAGE.captures(line) {
                    // Physique d'un côté, élémentaire et chaos de l'autre (moyenne par coup).
                    let key = if &m[1] == "Physical" { "phys_avg" } else { "ele_avg" };
                    let avg: f64 = DAMAGE_RANGE.captures_iter(&m[2]).map(|r| (num(&r[1]) + num(&r[2])) / 2.0).sum();
                    *item.props.entry(key.into()).or_insert(0.0) += avg;
                    if key == "phys_avg" {
                        if let Some(r) = DAMAGE_RANGE.captures(&m[2]) {
                            item.props.insert("phys_min".into(), num(&r[1]));
                            item.props.insert("phys_max".into(), num(&r[2]));
                        }
                    }
                } else if let Some(m) = WEAPON_RATE.captures(line) {
                    let key = if &m[1] == "Attacks per Second" { "aps" } else { "crit" };
                    item.props.insert(key.into(), num(&m[2]));
                }
            } else if is_flavour(section) {
                break;
            } else {
                item.mod_kinds.push(kind_from_suffix(line).unwrap_or(kind));
                item.mods.push(clean_mod(line));
            }
        }
    }
    Some(item)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const DUSK_VIGIL: &str = "Item Class: Staves
Rarity: Unique
Dusk Vigil
Ashen Staff
--------
Item Level: 8
--------
Grants Skill: Level 3 Firebolt
Grants Skill: Level 3 Ember Fusillade
--------
{ Unique Modifier — Damage, Caster }
120(80-120)% increased Spell Damage
{ Unique Modifier }
Trigger Ember Fusillade Skill on casting a Spell — Unscalable Value
{ Unique Modifier — Damage, Elemental, Fire }
Gain 31(30-50)% of Damage as Extra Fire Damage
--------
The candlemass tradition was born in a time of darkness and fear.";

    pub const GLOOM_TOUCH: &str = "Item Class: Gloves
Rarity: Rare
Gloom Touch
Torn Gloves
--------
Energy Shield: 33 (augmented)
--------
Requires: Level 6
--------
Item Level: 11
--------
{ Prefix Modifier \"Healthy\" (Tier: 8) — Life }
+21(20-29) to maximum Life
{ Suffix Modifier \"of the Cloud\" (Tier: 8) — Elemental, Lightning, Resistance }
+8(6-10)% to Lightning Resistance";

    pub const MANA_FLASK: &str = "Item Class: Mana Flasks
Rarity: Magic
Greater Mana Flask of the Constant
--------
Recovers 90 Mana over 2,50 Seconds
Consumes 10 of 70 Charges on use
Currently has 70 Charges
--------
Requires: Level 10
--------
Item Level: 10
--------
{ Suffix Modifier \"of the Constant\" (Tier: 6) }
29(23-30)% increased Charges gained
--------
Right click to drink. Can only hold charges while in belt. Refill at Wells or by killing monsters.";

    pub const STONE_CHARM: &str = "Item Class: Charms
Rarity: Normal
Stone Charm
--------
Lasts 3 Seconds
Consumes 20 of 40 Charges on use
Currently has 23 Charges
Cannot be Stunned
--------
Requires: Level 8
--------
Item Level: 8
--------
{ Implicit Modifier }
Used when you become Stunned — Unscalable Value
--------
Used automatically when condition is met. Can only hold charges while in belt. Refill at Wells or by killing monsters.";

    #[test]
    fn unique() {
        let it = parse(DUSK_VIGIL).unwrap();
        assert_eq!((it.item_class.as_str(), it.rarity, it.name.as_str(), it.base.as_str()),
                   ("Staves", Rarity::Unique, "Dusk Vigil", "Ashen Staff"));
        assert!(it.mods.contains(&"120% increased Spell Damage".to_string()));
        assert!(it.mods.contains(&"Trigger Ember Fusillade Skill on casting a Spell".to_string()));
        assert_eq!(it.granted_skills.len(), 2);
        assert!(!it.mods.iter().any(|m| m.contains("candlemass")));
    }

    #[test]
    fn props_and_requirements() {
        let it = parse(GLOOM_TOUCH).unwrap();
        assert_eq!(it.props["Energy Shield"], 33.0);
        assert_eq!(it.req_level, 6);
        assert_eq!(it.mods, vec!["+21 to maximum Life", "+8% to Lightning Resistance"]);
    }

    #[test]
    fn flask_with_decimal_comma() {
        let it = parse(MANA_FLASK).unwrap();
        assert_eq!(it.props["recover"], 90.0);
        assert_eq!(it.props["recover_time"], 2.5);
        assert_eq!(it.props["charges_max"], 70.0);
        assert_eq!(it.mods, vec!["29% increased Charges gained"]);
    }

    #[test]
    fn charm_keeps_trigger() {
        let it = parse(STONE_CHARM).unwrap();
        assert_eq!(it.props["duration"], 3.0);
        assert_eq!(it.mods, vec!["Cannot be Stunned", "Used when you become Stunned"]);
    }

    #[test]
    fn mod_kinds() {
        let belt = "Item Class: Belts\nRarity: Rare\nT\nRawhide Belt\n--------\nItem Level: 8\n--------\n{ Implicit Modifier — Charm }\nHas 1 Charm Slot\n--------\n{ Prefix Modifier \"Healthy\" (Tier: 9) — Life }\n+20(20-29) to maximum Life";
        let it = parse(belt).unwrap();
        assert_eq!(it.mod_kinds, vec![ModKind::Implicit, ModKind::Explicit]);
        let simple = parse("Item Class: Rings\nRarity: Rare\nX\nIron Ring\n--------\nItem Level: 8\n--------\n+5% to Cold Resistance (implicit)\n--------\n+20 to maximum Life").unwrap();
        assert_eq!(simple.mods, vec!["+5% to Cold Resistance", "+20 to maximum Life"]);
        assert_eq!(simple.mod_kinds, vec![ModKind::Implicit, ModKind::Explicit]);
    }

    #[test]
    fn not_an_item() {
        assert!(parse("mon mot de passe").is_none());
    }
}
