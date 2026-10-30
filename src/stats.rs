//! Fiche de caractéristiques : les mods de l'équipement porté (set d'armes affiché), additionnés.
//!
//! Attributs, défenses, vie, mana, esprit et résistances sont totalisés à part, mods hybrides
//! répartis (« to all Attributes », « to Fire and Cold Resistances »). Les autres mods sont
//! regroupés par texte, nombres additionnés. Les mods locaux d'une pièce d'armure sont déjà dans
//! ses défenses ; ceux d'une arme (dégâts ajoutés, vitesse d'attaque…) ne valent que pour elle et
//! sont listés à part.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::gear::{Gear, Slot};
use crate::guide::Step;

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+(?:\.\d+)?").unwrap());
static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((-?[\d.]+)-(-?[\d.]+)\)").unwrap());

/// Implicite d'une base tel que le catalogue le donne (« +(5-7) to all Attributes ») : chaque
/// fourchette remplacée par son milieu. Les implicites sans valeur pour la fiche (« -1 Prefix
/// Modifier allowed ») sont écartés.
fn implicit_line(line: &str) -> Option<String> {
    if line.contains("Modifier allowed") || line.contains("Modifiers allowed") {
        return None;
    }
    Some(
        RANGE
            .replace_all(line, |c: &regex::Captures| {
                let (a, b): (f64, f64) = (c[1].parse().unwrap_or(0.0), c[2].parse().unwrap_or(0.0));
                fmt((a + b) / 2.0)
            })
            .into_owned(),
    )
}

/// Total d'une caractéristique et d'où il vient : (emplacement, valeur).
#[derive(Default, Debug, Clone)]
pub struct Total {
    pub value: f64,
    pub from: Vec<(String, f64)>,
}

impl Total {
    fn add(&mut self, slot: &str, v: f64) {
        self.value += v;
        match self.from.iter_mut().find(|(s, _)| s == slot) {
            Some((_, total)) => *total += v,
            None => self.from.push((slot.to_string(), v)),
        }
    }
}

/// Mod additionné : texte avec « # » à la place des nombres, nombres additionnés, pièces.
#[derive(Debug, Clone)]
pub struct Line {
    pub template: String,
    pub values: Vec<f64>,
    pub from: Vec<String>,
}

impl Line {
    /// Les nombres seuls, avec leur signe et leur « % » : « +22% », « 5–10 ».
    pub fn value_text(&self) -> String {
        let parts: Vec<&str> = self.template.split('#').collect();
        self.values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let plus = if parts[i].ends_with('+') { "+" } else { "" };
                let pct = if parts.get(i + 1).is_some_and(|p| p.starts_with('%')) {
                    "%"
                } else {
                    ""
                };
                format!("{plus}{}{pct}", fmt(*v))
            })
            .collect::<Vec<_>>()
            .join("–")
    }

    /// Total pour comparer deux lignes de même texte.
    pub fn sum(&self) -> f64 {
        self.values.iter().sum()
    }

    /// Texte avec les nombres remis : « +86 to maximum Life ».
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut values = self.values.iter();
        for (i, part) in self.template.split('#').enumerate() {
            if i > 0 {
                out.push_str(&values.next().map(|v| fmt(*v)).unwrap_or_default());
            }
            out.push_str(part);
        }
        out
    }
}

/// Familles de mods, dans l'ordre d'affichage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Damage,
    Gems,
    Resources,
    Defence,
    Other,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Damage => tr!("Dégâts et vitesse"),
            Group::Gems => tr!("Niveaux de gemmes"),
            Group::Resources => tr!("Vie, mana et récupération"),
            Group::Defence => tr!("Défense"),
            Group::Other => tr!("Autres"),
        }
    }
}

#[derive(Default, Debug)]
pub struct Sheet {
    /// Force, dextérité, intelligence.
    pub attributes: [Total; 3],
    /// Armure, évasion, bouclier d'énergie.
    pub defences: [Total; 3],
    pub life: Total,
    pub mana: Total,
    pub spirit: Total,
    /// Feu, froid, foudre, chaos.
    pub resistances: [Total; 4],
    pub lines: BTreeMap<Group, Vec<Line>>,
    /// Mods locaux des armes : (emplacement, lignes).
    pub weapon_local: Vec<(String, Vec<String>)>,
    /// Pièces comptées.
    pub pieces: usize,
}

/// Emplacements qui comptent pour le set d'armes `set` (flacons et charmes exclus : leurs mods
/// ne valent qu'à l'usage).
fn worn_slots(set: u8) -> Vec<Slot> {
    let mut slots = vec![Slot::Weapon.in_set(set), Slot::Offhand.in_set(set)];
    slots.extend([
        Slot::Helmet,
        Slot::Body,
        Slot::Gloves,
        Slot::Boots,
        Slot::Belt,
        Slot::Amulet,
        Slot::RingLeft,
        Slot::RingRight,
    ]);
    slots
}

fn fmt(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round())
    } else {
        format!("{v:.1}")
    }
}

/// « +12 to Strength » -> (« +# to Strength », [12]).
fn split(line: &str) -> (String, Vec<f64>) {
    let values = NUMBER
        .find_iter(line)
        .filter_map(|m| m.as_str().parse().ok())
        .collect();
    (NUMBER.replace_all(line, "#").into_owned(), values)
}

fn is_armour_piece(slot: Slot, item_class: &str) -> bool {
    matches!(slot, Slot::Helmet | Slot::Body | Slot::Gloves | Slot::Boots)
        || (matches!(slot, Slot::Offhand | Slot::Offhand2)
            && ["Shield", "Buckler", "Foci", "Focus"]
                .iter()
                .any(|c| item_class.contains(c)))
}

/// Mod local d'une pièce d'armure : déjà compté dans ses défenses.
fn armour_local(line: &str) -> bool {
    !line.contains("Recharge")
        && [
            "increased Armour",
            "increased Evasion",
            "increased Energy Shield",
            "to Armour",
            "to Evasion Rating",
            "to maximum Energy Shield",
        ]
        .iter()
        .any(|k| line.contains(k))
}

/// Mod local d'une arme martiale : il ne vaut que pour ses propres coups.
fn weapon_local(line: &str) -> bool {
    line.starts_with("Adds ")
        || [
            "increased Physical Damage",
            "increased Attack Speed",
            "increased Critical Hit Chance",
            "to Accuracy Rating",
        ]
        .iter()
        .any(|k| line.contains(k))
}

fn group_of(template: &str) -> Group {
    let has = |keys: &[&str]| keys.iter().any(|k| template.contains(k));
    if has(&["Level of", "Skill Gems"]) {
        Group::Gems
    } else if has(&[
        "Life",
        "Mana",
        "Regeneration",
        "Leech",
        "Recover",
        "Flask",
        "Charm",
    ]) {
        Group::Resources
    } else if has(&[
        "Damage",
        "Speed",
        "Critical",
        "Penetrat",
        "Projectile",
        "Area of Effect",
        "Duration",
        "Accuracy",
    ]) && !has(&["taken", "Movement"])
    {
        Group::Damage
    } else if has(&[
        "Armour",
        "Evasion",
        "Energy Shield",
        "Block",
        "Resist",
        "Stun",
        "Ailment",
        "Freeze",
        "Ignite",
        "Shock",
        "Chill",
        "taken",
    ]) {
        Group::Defence
    } else {
        Group::Other
    }
}

/// Pièce à compter : une pièce portée, ou celle que vise le build.
pub struct Piece {
    pub slot: Slot,
    pub item_class: String,
    /// Défenses et esprit affichés par la pièce (mods locaux compris).
    pub props: BTreeMap<String, f64>,
    pub mods: Vec<String>,
    /// Pièce visée par le build : défenses de sa base, sur lesquelles appliquer ses mods locaux
    /// (les défenses affichées n'existent pas).
    pub base_defences: Option<[f64; 3]>,
}

const DEFENCE_NAMES: [&str; 3] = ["Armour", "Evasion", "Energy Shield"];

/// Défenses d'une base avec ses mods locaux : (base + ajouts) × (1 + augmentations).
fn local_defences(base: [f64; 3], mods: &[String]) -> [f64; 3] {
    let mut flat = [0.0; 3];
    let mut inc = [0.0; 3];
    for line in mods.iter().filter(|l| armour_local(l)) {
        let (_, values) = split(line);
        let Some(v) = values.first().copied() else {
            continue;
        };
        for (i, name) in DEFENCE_NAMES.iter().enumerate() {
            if line.contains(name) {
                if line.contains("increased") {
                    inc[i] += v;
                } else {
                    flat[i] += v;
                }
            }
        }
    }
    std::array::from_fn(|i| (base[i] + flat[i]) * (1.0 + inc[i] / 100.0))
}

/// Pénalité de résistances élémentaires selon la progression : -10 % par acte à partir de
/// l'acte 2 jusqu'à l'acte 4, puis selon le niveau de zone dans les interludes (le niveau du
/// personnage en tient lieu), -60 % dans l'endgame. Avec ce qui l'explique.
pub fn res_penalty(max_act: u32, level: u32) -> (f64, String) {
    match max_act {
        0 | 1 => (0.0, tr!("acte 1").into()),
        2..=4 => (-10.0 * (max_act - 1) as f64, tr!("acte {act}", act = max_act)),
        5 if level < 54 => (-30.0, tr!("interludes").into()),
        5 if level < 60 => (-40.0, tr!("interludes, zones 54-59").into()),
        5 if level < 65 => (-50.0, tr!("interludes, zones 60-64").into()),
        _ => (-60.0, tr!("endgame").into()),
    }
}

/// Plafond par défaut des résistances.
pub const RES_CAP: f64 = 75.0;

/// Totaux connus : true si la ligne a été comptée dans un total de la fiche.
fn add_known(sheet: &mut Sheet, slot: &str, template: &str, v: f64) -> bool {
    const ATTRS: [&str; 3] = ["Strength", "Dexterity", "Intelligence"];
    const RES: [&str; 3] = ["Fire", "Cold", "Lightning"];
    if let Some(rest) = template.strip_prefix("+# to ") {
        // Attributs : un, deux (« Strength and Dexterity ») ou tous.
        let attrs: Vec<usize> = if rest == "all Attributes" {
            vec![0, 1, 2]
        } else {
            let parts: Vec<&str> = rest.split(" and ").collect();
            let idx: Vec<usize> = parts
                .iter()
                .filter_map(|p| ATTRS.iter().position(|a| a == p))
                .collect();
            if idx.len() == parts.len() {
                idx
            } else {
                vec![]
            }
        };
        if !attrs.is_empty() {
            for i in attrs {
                sheet.attributes[i].add(slot, v);
            }
            return true;
        }
        match rest {
            "maximum Life" => sheet.life.add(slot, v),
            "maximum Mana" => sheet.mana.add(slot, v),
            "Spirit" => sheet.spirit.add(slot, v),
            "maximum Energy Shield" => sheet.defences[2].add(slot, v),
            "Armour" => sheet.defences[0].add(slot, v),
            "Evasion Rating" => sheet.defences[1].add(slot, v),
            _ => return false,
        }
        return true;
    }
    if let Some(rest) = template.strip_prefix("+#% to ") {
        if rest == "all Elemental Resistances" {
            for r in 0..3 {
                sheet.resistances[r].add(slot, v);
            }
            return true;
        }
        if rest == "Chaos Resistance" {
            sheet.resistances[3].add(slot, v);
            return true;
        }
        // « Fire Resistance », « Fire and Cold Resistances ».
        if let Some(names) = rest
            .strip_suffix(" Resistances")
            .or_else(|| rest.strip_suffix(" Resistance"))
        {
            let parts: Vec<&str> = names.split(" and ").collect();
            let idx: Vec<usize> = parts
                .iter()
                .filter_map(|p| RES.iter().position(|r| r == p))
                .collect();
            if !idx.is_empty() && idx.len() == parts.len() {
                for i in idx {
                    sheet.resistances[i].add(slot, v);
                }
                return true;
            }
        }
    }
    false
}

/// Fiche de l'équipement suivi, pour le set d'armes `set`.
pub fn sheet(gear: &Gear, set: u8) -> Sheet {
    let pieces: Vec<Piece> = worn_slots(set)
        .into_iter()
        .filter_map(|slot| {
            let it = gear.item(slot)?;
            Some(Piece {
                slot,
                item_class: it.item_class,
                props: it.props,
                mods: it.mods,
                base_defences: None,
            })
        })
        .collect();
    sheet_of(&pieces)
}

/// Fiche de ce que vise le build à cette étape, pour le set d'armes `set`. `defences` donne
/// les défenses d'une base, `class` sa classe d'objet, `implicits` ses implicites (milieu de
/// leur fourchette). Les uniques recommandés n'y sont pas (leurs mods ne sont pas connus ici).
pub fn build_sheet(
    step: &Step,
    set: u8,
    defences: impl Fn(&str) -> Option<[f64; 3]>,
    class: impl Fn(&str) -> Option<String>,
    implicits: impl Fn(&str) -> Vec<String>,
) -> Sheet {
    let slots = worn_slots(set);
    let pieces: Vec<Piece> = step
        .targets
        .iter()
        .filter(|(slot, _, _)| slots.contains(slot))
        .map(|(slot, base, targets)| Piece {
            slot: *slot,
            item_class: class(base).unwrap_or_default(),
            props: BTreeMap::new(),
            mods: implicits(base)
                .iter()
                .filter_map(|l| implicit_line(l))
                .chain(targets.iter().map(|t| t.line.clone()))
                .collect(),
            base_defences: Some(defences(base).unwrap_or_default()),
        })
        .collect();
    sheet_of(&pieces)
}

pub fn sheet_of(pieces: &[Piece]) -> Sheet {
    let mut sheet = Sheet {
        pieces: pieces.len(),
        ..Default::default()
    };
    let mut lines: BTreeMap<String, Line> = BTreeMap::new();
    for p in pieces {
        let name = match p.slot {
            Slot::RingLeft => tr!("Bague gauche"),
            Slot::RingRight => tr!("Bague droite"),
            s => s.label(),
        };
        let shown = match p.base_defences {
            Some(base) => local_defences(base, &p.mods),
            None => std::array::from_fn(|i| {
                p.props
                    .get(["Armour", "Evasion Rating", "Energy Shield"][i])
                    .copied()
                    .unwrap_or(0.0)
            }),
        };
        for (i, v) in shown.iter().enumerate() {
            if *v > 0.0 {
                sheet.defences[i].add(name, v.round());
            }
        }
        if let Some(v) = p.props.get("Spirit") {
            sheet.spirit.add(name, *v);
        }
        let armour = is_armour_piece(p.slot, &p.item_class);
        let weapon = matches!(p.slot, Slot::Weapon | Slot::Weapon2)
            || (matches!(p.slot, Slot::Offhand | Slot::Offhand2) && !armour);
        let mut local = Vec::new();
        for line in &p.mods {
            if armour && armour_local(line) {
                continue;
            }
            if weapon && weapon_local(line) {
                local.push(line.clone());
                continue;
            }
            let (template, values) = split(line);
            if values.len() == 1 && add_known(&mut sheet, name, &template, values[0]) {
                continue;
            }
            let entry = lines.entry(template.clone()).or_insert_with(|| Line {
                template,
                values: vec![0.0; values.len()],
                from: vec![],
            });
            if entry.values.len() == values.len() {
                for (t, v) in entry.values.iter_mut().zip(&values) {
                    *t += v;
                }
            }
            if !entry.from.iter().any(|f| f == name) {
                entry.from.push(name.to_string());
            }
        }
        if !local.is_empty() {
            sheet.weapon_local.push((name.to_string(), local));
        }
    }
    for line in lines.into_values() {
        sheet
            .lines
            .entry(group_of(&line.template))
            .or_default()
            .push(line);
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(slot: Slot, text: &str) -> Piece {
        let it = crate::item::parse(text).unwrap();
        Piece {
            slot,
            item_class: it.item_class,
            props: it.props,
            mods: it.mods,
            base_defences: None,
        }
    }

    #[test]
    fn totals_hybrids_and_locals() {
        let helmet = piece(
            Slot::Helmet,
            "Item Class: Helmets\nRarity: Rare\nDoom Crown\nHood\n--------\nEnergy Shield: 80 (augmented)\n--------\nItem Level: 60\n--------\n+40 to maximum Energy Shield\n+50 to maximum Life\n+20% to Fire and Cold Resistances\n+10 to Strength and Intelligence\n",
        );
        let ring = piece(
            Slot::RingLeft,
            "Item Class: Rings\nRarity: Rare\nGlyph Loop\nSapphire Ring\n--------\nItem Level: 60\n--------\n+20% to Cold Resistance (implicit)\n--------\n+30 to maximum Life\n+12% to all Elemental Resistances\n+25 to maximum Energy Shield\n10% increased Cast Speed\n",
        );
        let staff = piece(
            Slot::Weapon,
            "Item Class: Staves\nRarity: Rare\nHavoc Bane\nGelid Staff\n--------\nItem Level: 64\n--------\n119% increased Spell Damage\nAdds 5 to 10 Cold Damage\n+11 to Intelligence\n12% increased Cast Speed\n",
        );
        let s = sheet_of(&[helmet, ring, staff]);
        assert_eq!(s.life.value, 80.0);
        // Casque : 80 (le +40 local y est déjà) ; anneau : +25 global.
        assert_eq!(s.defences[2].value, 105.0);
        assert_eq!(
            s.resistances.iter().map(|r| r.value).collect::<Vec<_>>(),
            vec![32.0, 52.0, 12.0, 0.0]
        );
        assert_eq!(
            [
                s.attributes[0].value,
                s.attributes[1].value,
                s.attributes[2].value
            ],
            [10.0, 0.0, 21.0]
        );
        let cast = s.lines[&Group::Damage]
            .iter()
            .find(|l| l.template == "#% increased Cast Speed")
            .unwrap();
        assert_eq!(cast.text(), "22% increased Cast Speed");
        assert_eq!(cast.value_text(), "22%");
        assert_eq!(cast.from.len(), 2);
        assert_eq!(
            s.weapon_local,
            vec![(
                "Arme".to_string(),
                vec!["Adds 5 to 10 Cold Damage".to_string()]
            )]
        );
    }

    #[test]
    fn build_piece_applies_local_defences() {
        let p = Piece {
            slot: Slot::Helmet,
            item_class: String::new(),
            props: BTreeMap::new(),
            mods: vec![
                "+40 to maximum Energy Shield".into(),
                "50% increased Energy Shield".into(),
                "+60 to maximum Life".into(),
            ],
            base_defences: Some([0.0, 0.0, 100.0]),
        };
        let s = sheet_of(&[p]);
        assert_eq!(s.defences[2].value, 210.0);
        assert_eq!(s.life.value, 60.0);
    }

    #[test]
    fn implicit_ranges() {
        assert_eq!(
            implicit_line("+(5-7) to all Attributes").as_deref(),
            Some("+6 to all Attributes")
        );
        assert_eq!(implicit_line("-1 Prefix Modifier allowed"), None);
    }

    #[test]
    fn penalties() {
        assert_eq!(res_penalty(1, 10).0, 0.0);
        assert_eq!(res_penalty(3, 40).0, -20.0);
        assert_eq!(res_penalty(5, 57).0, -40.0);
        assert_eq!(res_penalty(6, 70).0, -60.0);
    }
}
