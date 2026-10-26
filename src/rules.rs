//! Valeur marchande d'un objet : le garder, le vendre au marché ou au PNJ.
//!
//! Devises et uniques : prix poe.ninja. Rares : heuristiques sur les mods recherchés
//! (le prix réel des rares viendra de l'API de trade). Chaque verdict liste les mods
//! qui l'ont déclenché.

use std::sync::LazyLock;

use regex::Regex;

use crate::item::{Item, Rarity};
use crate::prices::{MIN_LISTINGS, PriceData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Keep,
    Market,
    Check,
    Vendor,
}

impl Label {
    pub fn text(self) -> &'static str {
        match self {
            Label::Keep => "GARDER",
            Label::Market => "MARCHÉ",
            Label::Check => "À VÉRIFIER",
            Label::Vendor => "PNJ",
        }
    }

    /// Le conseil en clair (« à vendre au PNJ »).
    pub fn advice(self) -> &'static str {
        match self {
            Label::Keep => "à garder",
            Label::Market => "à mettre en vente au marché",
            Label::Check => "à vérifier",
            Label::Vendor => "à vendre au PNJ",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Value {
    pub label: Label,
    pub summary: String,
    pub reasons: Vec<String>,
}

const CASTER_WEAPONS: &[&str] = &["Staves", "Wands", "Foci", "Sceptres"];
const CURRENCY_CLASSES: &[&str] = &[
    "Stackable Currency", "Socketable", "Omen", "Delirium Instill", "Map Fragments", "Uncut Skill Gems",
    "Uncut Support Gems", "Uncut Spirit Gems", "Expedition Logbooks",
];

fn best(pattern: &str, mods: &[String]) -> Option<f64> {
    let re = Regex::new(pattern).unwrap();
    mods.iter().filter_map(|m| re.captures(m)).filter_map(|c| c[1].parse::<f64>().ok()).reduce(f64::max)
}

static RES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(\d+)% to (Fire|Cold|Lightning|Chaos) Resistance$").unwrap());
static ALL_RES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(\d+)% to all Elemental Resistances$").unwrap());

fn resistances(mods: &[String]) -> f64 {
    mods.iter()
        .map(|m| {
            if let Some(c) = RES.captures(m) {
                c[1].parse().unwrap_or(0.0)
            } else if let Some(c) = ALL_RES.captures(m) {
                3.0 * c[1].parse::<f64>().unwrap_or(0.0)
            } else {
                0.0
            }
        })
        .sum()
}

/// (points marché, points build, raisons) pour un rare ou un magique.
fn score_rare(it: &Item) -> (u32, u32, Vec<String>) {
    let (mods, cls) = (&it.mods, it.item_class.as_str());
    let (mut market, mut build, mut why) = (0u32, 0u32, Vec::new());

    if let Some(n) = best(r"^\+(\d+) to Level of all Spell Skills$", mods) {
        market += 2 * n as u32;
        build += 2 * n as u32;
        why.push(format!("+{n} niveaux de sorts"));
    }
    if let Some(n) = best(r"^\+(\d+) to Level of all Cold Spell Skills$", mods) {
        market += n as u32;
        build += 2 * n as u32;
        why.push(format!("+{n} niveaux de sorts de froid"));
    }
    if let Some(n) = best(r"^\+(\d+) to Level of all (?:Minion|Projectile|Melee|Attack|Physical|Fire|Lightning|Chaos)(?: Spell)? Skills$", mods) {
        market += 2 * n as u32;
        why.push(format!("+{n} niveaux de compétences (recherché par d'autres builds)"));
    }
    if CASTER_WEAPONS.contains(&cls) {
        if let Some(v) = best(r"^(\d+)% increased Spell Damage$", mods).filter(|v| *v >= 60.0) {
            market += 2;
            build += 2;
            why.push(format!("{v}% dégâts de sort"));
        }
        if let Some(v) = best(r"^(\d+)% increased Cold Damage$", mods).filter(|v| *v >= 60.0) {
            build += 2;
            why.push(format!("{v}% dégâts de froid"));
        }
        if let Some(v) = best(r"^(\d+)% increased Critical Hit Chance for Spells$", mods).filter(|v| *v >= 40.0) {
            market += 1;
            build += 1;
            why.push(format!("{v}% critique des sorts"));
        }
        if let Some(v) = best(r"^(\d+)% increased Cast Speed$", mods).filter(|v| *v >= 15.0) {
            market += 1;
            build += 1;
            why.push(format!("{v}% vitesse d'incantation"));
        }
    }
    // Mods génériques : ils font la valeur marchande, la comparaison dit s'ils t'améliorent.
    if cls == "Boots" {
        if let Some(ms) = best(r"^(\d+)% increased Movement Speed$", mods) {
            if ms >= 30.0 {
                market += 4;
                why.push(format!("{ms}% vitesse de déplacement"));
            } else if ms >= 25.0 {
                market += 2;
                why.push(format!("{ms}% vitesse de déplacement"));
            }
        }
    }
    if let Some(v) = best(r"^\+(\d+) to maximum Life$", mods).filter(|v| *v >= 80.0) {
        market += 1;
        why.push(format!("+{v} vie"));
    }
    let res = resistances(mods);
    if res >= 60.0 {
        market += 1 + u32::from(res >= 90.0);
        why.push(format!("{res}% de résistances cumulées"));
    }
    if let Some(v) = best(r"^\+(\d+) to Spirit$", mods).filter(|v| *v >= 30.0) {
        market += 2;
        build += 1;
        why.push(format!("+{v} esprit"));
    }
    if it.item_level >= 81 {
        market += 1;
        why.push(format!("base niveau {} (craft)", it.item_level));
    }
    (market, build, why)
}

pub fn evaluate(it: &Item, prices: Option<&PriceData>, market_min_ex: f64) -> Value {
    let value = |label, summary: String, reasons| Value { label, summary, reasons };
    if it.rarity == Rarity::Currency || CURRENCY_CLASSES.contains(&it.item_class.as_str()) {
        let Some((p, unit)) = prices.and_then(|p| Some((p, p.currency(&it.base)?))) else {
            return value(Label::Keep, "devise (prix inconnu)".into(), vec![]);
        };
        let total = unit * it.stack as f64;
        let label = if p.to_ex(total) >= market_min_ex { Label::Market } else { Label::Keep };
        return value(label, format!("{} l'unité, {} le lot", p.fmt(unit), p.fmt(total)), vec![]);
    }
    if it.rarity == Rarity::Unique {
        let Some((p, (v, listings))) = prices.and_then(|p| Some((p, p.unique(&it.name, &it.base)?))) else {
            let why = if prices.is_none() { "prix en cours de chargement" } else { "unique sans prix connu (rare sur le marché ?)" };
            return value(Label::Check, why.into(), vec![]);
        };
        let note = format!("≈ {} ({listings} annonces)", p.fmt(v));
        if listings < MIN_LISTINGS {
            return value(Label::Check, format!("{note}, prix peu fiable"), vec![]);
        }
        if p.to_ex(v) >= market_min_ex {
            return value(Label::Market, note, vec![]);
        }
        return value(Label::Vendor, format!("{note} — ne vaut rien au marché"), vec![]);
    }
    if it.unidentified {
        return value(Label::Check, "non identifié : identifie-le pour juger ses mods".into(), vec![]);
    }
    if matches!(it.rarity, Rarity::Rare | Rarity::Magic) {
        let (market, build, why) = score_rare(it);
        let (label, summary) = if build >= 4 {
            (Label::Keep, "mods intéressants pour ton build")
        } else if market >= 4 {
            (Label::Market, "mods recherchés, vérifie le prix au marché")
        } else if market >= 2 {
            (Label::Check, "quelques bons mods, peut se vendre")
        } else {
            (Label::Vendor, "ne vaut rien au marché")
        };
        return value(label, summary.into(), why);
    }
    value(Label::Vendor, "objet normal, sans valeur au marché".into(), vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{self, tests::DUSK_VIGIL};

    fn prices() -> PriceData {
        let mut p = PriceData { exalted_per_divine: 693.0, ..Default::default() };
        p.uniques.insert("Dusk Vigil|Ashen Staff".into(), (0.003, 6013));
        p.currency.insert("Exalted Orb".into(), 1.0 / 693.0);
        p
    }

    #[test]
    fn cheap_unique_is_vendor() {
        let v = evaluate(&item::parse(DUSK_VIGIL).unwrap(), Some(&prices()), 10.0);
        assert_eq!(v.label, Label::Vendor);
    }

    #[test]
    fn movement_boots_go_to_market() {
        let boots = "Item Class: Boots\nRarity: Rare\nGale\nPadded Leggings\n--------\nItem Level: 60\n--------\n30% increased Movement Speed\n+35% to Fire Resistance\n+32% to Cold Resistance\n+85 to maximum Life";
        assert_eq!(evaluate(&item::parse(boots).unwrap(), Some(&prices()), 10.0).label, Label::Market);
    }

    #[test]
    fn caster_staff_is_kept() {
        let staff = "Item Class: Staves\nRarity: Rare\nDoom\nGelid Staff\n--------\nItem Level: 52\n--------\n+3 to Level of all Spell Skills\n72% increased Spell Damage";
        assert_eq!(evaluate(&item::parse(staff).unwrap(), Some(&prices()), 10.0).label, Label::Keep);
    }

    #[test]
    fn currency_stack_value() {
        let ex = "Item Class: Stackable Currency\nRarity: Currency\nExalted Orb\n--------\nStack Size: 20/20\n--------\nAugments a Rare item";
        let v = evaluate(&item::parse(ex).unwrap(), Some(&prices()), 10.0);
        assert_eq!(v.label, Label::Market);
    }
}
