//! Équipement suivi : quelle pièce est dans quel emplacement.
//!
//! On ne lit pas l'équipement dans le jeu : tu enregistres chaque pièce avec Ctrl+Alt+C.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::compare;
use crate::item::{self, Item};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Slot {
    Weapon,
    Offhand,
    Helmet,
    Body,
    Gloves,
    Boots,
    Belt,
    Amulet,
    RingLeft,
    RingRight,
    LifeFlask,
    ManaFlask,
    Charm1,
    Charm2,
    Charm3,
}

pub const RINGS: [Slot; 2] = [Slot::RingLeft, Slot::RingRight];
pub const CHARMS: [Slot; 3] = [Slot::Charm1, Slot::Charm2, Slot::Charm3];

impl Slot {
    pub const ALL: [Slot; 15] = [
        Slot::Weapon, Slot::Offhand, Slot::Helmet, Slot::Body, Slot::Gloves, Slot::Boots,
        Slot::Belt, Slot::Amulet, Slot::RingLeft, Slot::RingRight, Slot::LifeFlask,
        Slot::ManaFlask, Slot::Charm1, Slot::Charm2, Slot::Charm3,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Slot::Weapon => "Arme",
            Slot::Offhand => "Main gauche",
            Slot::Helmet => "Casque",
            Slot::Body => "Torse",
            Slot::Gloves => "Gants",
            Slot::Boots => "Bottes",
            Slot::Belt => "Ceinture",
            Slot::Amulet => "Amulette",
            Slot::RingLeft | Slot::RingRight => "Anneau",
            Slot::LifeFlask => "Fiole de vie",
            Slot::ManaFlask => "Fiole de mana",
            Slot::Charm1 | Slot::Charm2 | Slot::Charm3 => "Charme",
        }
    }
}

/// Type d'emplacement d'une classe d'objet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Single(Slot),
    TwoHanded,
    Ring,
    Charm,
}

pub fn kind_of(item_class: &str) -> Option<Kind> {
    Some(match item_class {
        "Helmets" => Kind::Single(Slot::Helmet),
        "Body Armours" => Kind::Single(Slot::Body),
        "Gloves" => Kind::Single(Slot::Gloves),
        "Boots" => Kind::Single(Slot::Boots),
        "Belts" => Kind::Single(Slot::Belt),
        "Amulets" => Kind::Single(Slot::Amulet),
        "Rings" => Kind::Ring,
        "Staves" | "Quarterstaves" | "Two Hand Swords" | "Two Hand Axes" | "Two Hand Maces"
        | "Bows" | "Crossbows" => Kind::TwoHanded,
        "Wands" | "Sceptres" | "One Hand Swords" | "One Hand Axes" | "One Hand Maces" | "Daggers"
        | "Claws" | "Spears" | "Flails" => Kind::Single(Slot::Weapon),
        "Foci" | "Shields" | "Bucklers" | "Quivers" => Kind::Single(Slot::Offhand),
        "Life Flasks" => Kind::Single(Slot::LifeFlask),
        "Mana Flasks" => Kind::Single(Slot::ManaFlask),
        "Charms" => Kind::Charm,
        _ => return None,
    })
}

/// Résultat d'un enregistrement.
#[derive(Debug, PartialEq)]
pub enum Equipped {
    Placed { slot: Slot, replaced: Option<String> },
    AlreadyThere(Slot),
    CharmsFull,
    UnknownClass(String),
}

#[derive(Default, Serialize, Deserialize)]
pub struct Gear {
    /// Emplacement -> texte brut de l'objet.
    pub slots: BTreeMap<Slot, String>,
    #[serde(skip)]
    path: PathBuf,
}

impl Gear {
    pub fn load(path: &Path) -> Gear {
        let mut gear: Gear = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        gear.path = path.to_path_buf();
        gear
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(self).unwrap())
    }

    pub fn item(&self, slot: Slot) -> Option<Item> {
        self.slots.get(&slot).and_then(|t| item::parse(t))
    }

    /// L'arme suivie se tient à deux mains (la main gauche est alors prise).
    pub fn two_handed(&self) -> bool {
        self.item(Slot::Weapon).is_some_and(|w| kind_of(&w.item_class) == Some(Kind::TwoHanded))
    }

    /// Emplacements de charme ouverts par la ceinture suivie ("Has 1 Charm Slot").
    /// Les bonus de quête (+1 emplacement à l'acte 2) ne sont pas visibles d'ici.
    pub fn charm_slots(&self) -> Option<usize> {
        let belt = self.item(Slot::Belt)?;
        Some(
            belt.mods
                .iter()
                .filter_map(|m| CHARM_SLOTS.captures(m))
                .map(|c| c[1].parse::<usize>().unwrap_or(0))
                .sum(),
        )
    }

    /// Emplacements qu'on s'attend à voir remplis mais qui ne sont pas suivis.
    pub fn untracked(&self) -> Vec<Slot> {
        let charms = self.charm_slots().unwrap_or(0);
        Slot::ALL
            .into_iter()
            .filter(|s| !self.slots.contains_key(s))
            .filter(|s| !(*s == Slot::Offhand && self.two_handed()))
            .filter(|s| CHARMS.iter().position(|c| c == s).is_none_or(|i| i < charms))
            .collect()
    }

    pub fn clear(&mut self, slot: Slot) {
        self.slots.remove(&slot);
    }

    pub fn equip(&mut self, it: &Item) -> Equipped {
        let Some(kind) = kind_of(&it.item_class) else {
            return Equipped::UnknownClass(it.item_class.clone());
        };
        if let Some((&slot, _)) = self.slots.iter().find(|(_, t)| **t == it.raw) {
            return Equipped::AlreadyThere(slot);
        }
        let slot = match kind {
            Kind::Single(s) => s,
            Kind::TwoHanded => {
                self.slots.remove(&Slot::Offhand);
                Slot::Weapon
            }
            Kind::Ring => {
                // Case libre d'abord ; sinon on suppose que tu as remplacé le moins bon anneau.
                match RINGS.iter().find(|s| !self.slots.contains_key(s)) {
                    Some(&s) => s,
                    None => *RINGS
                        .iter()
                        .min_by(|a, b| {
                            let sa = self.item(**a).map(|i| compare::score(&i)).unwrap_or(0.0);
                            let sb = self.item(**b).map(|i| compare::score(&i)).unwrap_or(0.0);
                            sa.total_cmp(&sb)
                        })
                        .unwrap(),
                }
            }
            Kind::Charm => {
                // Chaque charme contre une menace précise : on remplace celui du même type.
                let same = CHARMS.iter().find(|s| self.item(**s).is_some_and(|c| c.base == it.base));
                match same.or_else(|| CHARMS.iter().find(|s| !self.slots.contains_key(s))) {
                    Some(&s) => s,
                    None => return Equipped::CharmsFull,
                }
            }
        };
        let replaced = self.item(slot).map(|old| old.title());
        self.slots.insert(slot, it.raw.clone());
        Equipped::Placed { slot, replaced }
    }
}

static CHARM_SLOTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:Has |\+)(\d+) Charm Slots?$").unwrap());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::tests::{DUSK_VIGIL, GLOOM_TOUCH, STONE_CHARM};

    fn ring(name: &str, life: u32) -> Item {
        item::parse(&format!(
            "Item Class: Rings\nRarity: Rare\n{name}\nIron Ring\n--------\nItem Level: 20\n--------\n+{life} to maximum Life"
        ))
        .unwrap()
    }

    fn gear() -> Gear {
        Gear { slots: BTreeMap::new(), path: PathBuf::new() }
    }

    #[test]
    fn single_slot_and_replacement() {
        let mut g = gear();
        let gloves = item::parse(GLOOM_TOUCH).unwrap();
        assert_eq!(g.equip(&gloves), Equipped::Placed { slot: Slot::Gloves, replaced: None });
        assert_eq!(g.equip(&gloves), Equipped::AlreadyThere(Slot::Gloves));
    }

    #[test]
    fn rings_fill_then_replace_the_worse() {
        let mut g = gear();
        g.equip(&ring("Good", 60));
        g.equip(&ring("Weak", 10));
        let r = g.equip(&ring("New", 30));
        assert_eq!(r, Equipped::Placed { slot: Slot::RingRight, replaced: Some("Weak (Iron Ring)".into()) });
    }

    #[test]
    fn two_hander_clears_offhand() {
        let mut g = gear();
        g.slots.insert(Slot::Offhand, "x".into());
        g.equip(&item::parse(DUSK_VIGIL).unwrap());
        assert!(!g.slots.contains_key(&Slot::Offhand));
        assert!(g.slots.contains_key(&Slot::Weapon));
    }

    #[test]
    fn untracked_ignores_offhand_of_two_hander_and_closed_charm_slots() {
        let mut g = gear();
        g.equip(&item::parse(DUSK_VIGIL).unwrap());
        g.equip(&item::parse(
            "Item Class: Belts\nRarity: Rare\nTether\nRawhide Belt\n--------\nItem Level: 8\n--------\n{ Implicit Modifier — Charm }\nHas 1 Charm Slot",
        ).unwrap());
        assert_eq!(g.charm_slots(), Some(1));
        let missing = g.untracked();
        assert!(!missing.contains(&Slot::Offhand));
        assert!(missing.contains(&Slot::Charm1));
        assert!(!missing.contains(&Slot::Charm2));
    }

    #[test]
    fn charms_by_type() {
        let mut g = gear();
        let stone = item::parse(STONE_CHARM).unwrap();
        assert_eq!(g.equip(&stone), Equipped::Placed { slot: Slot::Charm1, replaced: None });
        let stone2 = item::parse(&STONE_CHARM.replace("Lasts 3", "Lasts 5")).unwrap();
        assert_eq!(g.equip(&stone2), Equipped::Placed { slot: Slot::Charm1, replaced: Some("Stone Charm".into()) });
    }
}
