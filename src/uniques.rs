//! Uniques recommandés par le build : où ils vont, lesquels sont déjà portés, où les chercher.

use std::collections::HashSet;

use crate::gear::{Gear, Slot, CHARMS, RINGS};
use crate::guide::{Step, Wanted};
use crate::item::{self, Rarity};

/// Un unique du build et les étapes qui le recommandent.
pub struct Summary {
    pub name: String,
    pub slot: Option<Slot>,
    pub steps: Vec<String>,
}

/// Tous les uniques du build, du plus partagé entre étapes au moins partagé (à égalité,
/// dans l'ordre d'apparition).
pub fn summary(steps: &[Step]) -> Vec<Summary> {
    let mut out: Vec<Summary> = Vec::new();
    for step in steps {
        for w in &step.uniques {
            match out.iter_mut().find(|s| s.name == w.name) {
                Some(s) if s.steps.contains(&step.label) => {}
                Some(s) => s.steps.push(step.label.clone()),
                None => out.push(Summary { name: w.name.clone(), slot: w.slot, steps: vec![step.label.clone()] }),
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.steps.len()));
    out
}

/// Uniques d'une étape, sans doublon : nom, emplacement, nombre d'exemplaires.
pub fn of_step(step: &Step) -> Vec<(&Wanted, usize)> {
    let mut out: Vec<(&Wanted, usize)> = Vec::new();
    for w in &step.uniques {
        match out.iter_mut().find(|(o, _)| o.name == w.name) {
            Some((_, n)) => *n += 1,
            None => out.push((w, 1)),
        }
    }
    out
}

/// Emplacement réel : une fiole de mana est rangée par le planificateur avec les fioles de vie,
/// son type de base (poe.ninja) la remet à sa place.
pub fn slot(slot: Option<Slot>, base: Option<&str>) -> Option<Slot> {
    match (slot, base) {
        (Some(Slot::LifeFlask), Some(b)) if b.contains("Mana Flask") => Some(Slot::ManaFlask),
        (s, _) => s,
    }
}

pub fn slot_label(slot: Option<Slot>) -> &'static str {
    slot.map(Slot::label).unwrap_or("?")
}

/// Noms des uniques de l'équipement suivi.
pub fn worn(gear: &Gear) -> HashSet<String> {
    gear.slots.values().filter_map(|t| item::parse(t)).filter(|i| i.rarity == Rarity::Unique).map(|i| i.name).collect()
}

/// Cases à marquer d'une étoile : uniques de l'étape pas encore portés, chacun sur sa case
/// (un unique en double, une arme par set, compte pour chaque case). Les charmes prennent
/// les cases de charme ouvertes qui n'en portent pas déjà un du build.
pub fn missing_by_slot(step: &Step, gear: &Gear, base_of: impl Fn(&str) -> Option<String>) -> Vec<(Slot, String)> {
    let name_in = |s: Slot| gear.item(s).filter(|i| i.rarity == Rarity::Unique).map(|i| i.name);
    let build_names: HashSet<&str> = step.uniques.iter().map(|w| w.name.as_str()).collect();
    let open_charms = gear.charm_slots().unwrap_or(CHARMS.len()).min(CHARMS.len());
    let mut free_charms = CHARMS[..open_charms].iter().filter(|c| !name_in(**c).is_some_and(|n| build_names.contains(n.as_str())));
    let mut out: Vec<(Slot, String)> = Vec::new();
    for w in &step.uniques {
        let target = match slot(w.slot, base_of(&w.name).as_deref()) {
            Some(Slot::Charm1) => {
                if CHARMS.iter().any(|c| name_in(*c).as_deref() == Some(w.name.as_str())) {
                    continue;
                }
                free_charms.next().copied()
            }
            // Anneaux : l'un ou l'autre fait l'affaire.
            Some(s @ (Slot::RingLeft | Slot::RingRight)) => {
                if RINGS.iter().any(|r| name_in(*r).as_deref() == Some(w.name.as_str())) {
                    continue;
                }
                Some(s)
            }
            s => s,
        };
        let Some(t) = target else { continue };
        if name_in(t).as_deref() != Some(w.name.as_str()) && !out.iter().any(|(s, n)| *s == t && *n == w.name) {
            out.push((t, w.name.clone()));
        }
    }
    out
}

/// Recherche de l'unique sur le site de trade (vendeurs en ligne, moins cher d'abord).
pub fn trade_url(league: &str, name: &str, base: Option<&str>) -> String {
    let mut query = serde_json::json!({ "status": { "option": "online" }, "name": name });
    if let Some(b) = base {
        query["type"] = b.into();
    }
    let q = serde_json::json!({ "query": query, "sort": { "price": "asc" } }).to_string();
    format!("https://www.pathofexile.com/trade2/search/poe2/{}?q={}", encode(league), encode(&q))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(label: &str, uniques: &[(&str, Option<Slot>)]) -> Step {
        Step {
            label: label.into(),
            skills: vec![],
            passives: vec![],
            targets: vec![],
            uniques: uniques.iter().map(|(n, s)| Wanted { name: n.to_string(), slot: *s }).collect(),
            passive_sets: Default::default(),
        }
    }

    #[test]
    fn shared_uniques_come_first() {
        let steps = [
            step("BUDGET", &[("Sadist's Mercy", Some(Slot::Weapon))]),
            step("BRUTUS", &[("Brutus' Lead Sprinkler", Some(Slot::Weapon)), ("Svalinn", Some(Slot::Offhand))]),
            step("94% Block", &[("Sadist's Mercy", Some(Slot::Weapon)), ("Sadist's Mercy", None), ("Svalinn", Some(Slot::Offhand))]),
        ];
        let s = summary(&steps);
        assert_eq!(s[0].name, "Sadist's Mercy");
        assert_eq!(s[0].steps, ["BUDGET", "94% Block"]);
        assert_eq!(s[1].name, "Svalinn");
        assert_eq!(of_step(&steps[2])[0].1, 2);
    }

    #[test]
    fn mana_flask_goes_to_its_slot_and_worn_uniques_are_skipped() {
        let st = step("x", &[("Lavianga's Spirits", Some(Slot::LifeFlask)), ("For Utopia", Some(Slot::Charm1)), ("Mystère", None)]);
        let g = Gear::default();
        let base = |n: &str| (n == "Lavianga's Spirits").then(|| "Gargantuan Mana Flask".to_string());
        let m = missing_by_slot(&st, &g, base);
        assert_eq!(m, vec![(Slot::ManaFlask, "Lavianga's Spirits".into()), (Slot::Charm1, "For Utopia".into())]);
    }

    #[test]
    fn one_star_per_weapon_set() {
        let st = step("x", &[("Sadist's Mercy", Some(Slot::Weapon)), ("Sadist's Mercy", Some(Slot::Weapon2))]);
        let m = missing_by_slot(&st, &Gear::default(), |_| None);
        assert_eq!(m, vec![(Slot::Weapon, "Sadist's Mercy".into()), (Slot::Weapon2, "Sadist's Mercy".into())]);
    }

    #[test]
    fn trade_link_is_encoded() {
        let url = trade_url("Forbidden Rites", "Brutus' Lead Sprinkler", Some("Ornate Mace"));
        assert!(url.starts_with("https://www.pathofexile.com/trade2/search/poe2/Forbidden%20Rites?q=%7B%22query%22"));
        assert!(!url[8..].contains(' ') && !url.contains('\''));
    }
}
