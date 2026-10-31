//! Export du personnage en Markdown, à coller dans une conversation avec une IA : les sections
//! choisies dans la fenêtre d'export, chacune sous son titre.

use serde::{Deserialize, Serialize};

use crate::stats::{Group, Sheet, Total, RES_CAP};

/// Sections cochées dans la fenêtre d'export (gardées dans la config).
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Sections {
    pub gear: bool,
    pub stats: bool,
    pub passives: bool,
    pub jewels: bool,
    pub atlas: bool,
    pub maps: bool,
    pub quests: bool,
}

impl Default for Sections {
    fn default() -> Self {
        // Les maps, la section la plus longue, seulement à la demande.
        Sections { gear: true, stats: true, passives: true, jewels: true, atlas: true, maps: false, quests: true }
    }
}

impl Sections {
    /// Chaque section, son nom et ce qu'elle contient, pour les cases de la fenêtre.
    pub fn items(&mut self) -> [(&mut bool, &'static str, &'static str); 7] {
        [
            (&mut self.gear, tr!("Équipement"), tr!("chaque pièce suivie, telle que copiée en jeu")),
            (&mut self.stats, tr!("Caractéristiques"), tr!("totaux et leurs sources, porté face au build")),
            (&mut self.passives, tr!("Talents"), tr!("ton arbre réel et l'écart avec le build")),
            (&mut self.jewels, tr!("Joyaux"), tr!("joyaux suivis et leur châsse")),
            (&mut self.atlas, tr!("Atlas"), tr!("passifs de l'Atlas pris")),
            (&mut self.maps, tr!("Maps"), tr!("résumé et dernières maps")),
            (&mut self.quests, tr!("Quêtes"), tr!("récompenses obtenues et choix faits")),
        ]
    }
}

pub fn fmt(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round())
    } else {
        format!("{v:.1}")
    }
}

/// Sources d'un total : « Classe 7, Casque 27, Arbre de talents 10 », puis le pourcentage
/// augmenté et ses sources.
fn sources(t: &Total) -> String {
    let mut out: Vec<String> = t.from.iter().map(|(s, v)| format!("{s} {}", fmt(*v))).collect();
    if !t.inc_from.is_empty() {
        let inc: Vec<String> = t.inc_from.iter().map(|(s, v)| format!("{s} {} %", fmt(*v))).collect();
        out.push(tr!("puis × {k} ({inc})", k = format!("{:.2}", 1.0 + t.inc / 100.0), inc = inc.join(", ")));
    }
    out.join(", ")
}

/// Caractéristiques : tableau porté / build, sources des totaux portés, puis les autres mods
/// additionnés.
pub fn stats(worn: &Sheet, build: Option<&Sheet>, build_title: &str, set: u8, penalty: &(f64, String), tree_known: bool) -> String {
    let mut out = String::new();
    let set = if set == 2 { "II" } else { "I" };
    out.push_str(&format!("{}\n\n", tr!("Set d'armes {set}.", set)));
    out.push_str(&format!("| | {} | {build_title} |\n|---|---|---|\n", tr!("Porté")));
    let totals: [(&str, fn(&Sheet) -> &Total); 9] = [
        (tr!("Force"), |s| &s.attributes[0]),
        (tr!("Dextérité"), |s| &s.attributes[1]),
        (tr!("Intelligence"), |s| &s.attributes[2]),
        (tr!("Vie"), |s| &s.life),
        (tr!("Mana"), |s| &s.mana),
        (tr!("Esprit"), |s| &s.spirit),
        (tr!("Armure"), |s| &s.defences[0]),
        (tr!("Score d'Évasion"), |s| &s.defences[1]),
        (tr!("Bouclier d'énergie"), |s| &s.defences[2]),
    ];
    for (name, pick) in totals {
        let b = build.map_or("—".to_string(), |b| fmt(pick(b).value));
        out.push_str(&format!("| {name} | {} | {b} |\n", fmt(pick(worn).value)));
    }
    // Résistances comme en jeu : effective (avant plafond).
    let res = |t: &Total, i: usize| {
        let total = t.value + if i < 3 { penalty.0 } else { 0.0 };
        format!("{} % ({} %)", fmt(total.min(RES_CAP)), fmt(total))
    };
    for (i, name) in [tr!("Feu"), tr!("Froid"), tr!("Foudre"), tr!("Chaos")].into_iter().enumerate() {
        let b = build.map_or("—".to_string(), |b| res(&b.resistances[i], i));
        out.push_str(&format!("| {} | {} | {b} |\n", tr!("Résistance : {name}", name), res(&worn.resistances[i], i)));
    }
    out.push_str(&format!("\n{}\n", tr!("Résistances : effective (avant plafond), pénalité {when} {n} % comprise.", when = penalty.1, n = fmt(penalty.0))));
    if !tree_known {
        out.push_str(&format!("{}\n", tr!("Ton arbre de talents n'est pas compté (journal du jeu incomplet).")));
    }
    out.push_str(&format!("\n### {}\n\n", tr!("Sources (porté)")));
    let named: [(&str, &Total); 9] = [
        (tr!("Force"), &worn.attributes[0]),
        (tr!("Dextérité"), &worn.attributes[1]),
        (tr!("Intelligence"), &worn.attributes[2]),
        (tr!("Vie"), &worn.life),
        (tr!("Mana"), &worn.mana),
        (tr!("Esprit"), &worn.spirit),
        (tr!("Bouclier d'énergie"), &worn.defences[2]),
        (tr!("Armure"), &worn.defences[0]),
        (tr!("Score d'Évasion"), &worn.defences[1]),
    ];
    for (name, t) in named.into_iter().filter(|(_, t)| !t.from.is_empty()) {
        out.push_str(&format!("- {name} {} : {}\n", fmt(t.value), sources(t)));
    }
    for (i, name) in [tr!("Feu"), tr!("Froid"), tr!("Foudre"), tr!("Chaos")].into_iter().enumerate() {
        let t = &worn.resistances[i];
        if !t.from.is_empty() {
            out.push_str(&format!("- {} : {}\n", tr!("Résistance : {name}", name), sources(t)));
        }
    }
    // Autres mods : texte du jeu (« # » à la place des nombres), porté puis build.
    out.push_str(&format!("\n### {}\n\n", tr!("Autres mods additionnés (porté / build)")));
    for group in [Group::Damage, Group::Gems, Group::Resources, Group::Defence, Group::Other] {
        let mine = worn.lines.get(&group).map(Vec::as_slice).unwrap_or_default();
        let theirs = build.and_then(|b| b.lines.get(&group)).map(Vec::as_slice).unwrap_or_default();
        let mut templates: Vec<&str> = mine.iter().chain(theirs).map(|l| l.template.as_str()).collect();
        templates.sort();
        templates.dedup();
        if templates.is_empty() {
            continue;
        }
        out.push_str(&format!("**{}**\n\n", group.label()));
        for t in templates {
            let a = mine.iter().find(|l| l.template == t).map_or("—".to_string(), |l| l.value_text());
            let b = theirs.iter().find(|l| l.template == t).map_or("—".to_string(), |l| l.value_text());
            out.push_str(&format!("- `{t}` : {a} / {b}\n"));
        }
        out.push('\n');
    }
    if !worn.weapon_local.is_empty() {
        out.push_str(&format!("**{}**\n\n", tr!("Propre à l'arme portée")));
        for (slot, lines) in &worn.weapon_local {
            out.push_str(&format!("- {slot} : {}\n", lines.join(" ; ")));
        }
    }
    out
}

/// Liste de noms en une ligne, « — » si vide.
pub fn names(list: &[String]) -> String {
    if list.is_empty() { "—".to_string() } else { list.join(", ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_table_and_sources() {
        let mut s = Sheet::default();
        crate::stats::add_source(&mut s, "Casque", &["+100 to maximum Life".to_string(), "+40% to Fire Resistance".to_string(), "10% increased Spell Damage".to_string()]);
        crate::stats::add_source(&mut s, "Quêtes", &["5% increased maximum Life".to_string()]);
        crate::stats::finish(&mut s, 10, "Sorceress");
        let md = stats(&s, None, "Build", 1, &(-10.0, "acte 2".into()), true);
        // (16 + 120 + 14 + 100) × 1,05 = 262,5 → 263.
        assert!(md.contains("| Vie | 263 | — |"), "{md}");
        assert!(md.contains("30 % (30 %)"), "{md}");
        assert!(md.contains("Casque 100"), "{md}");
        assert!(md.contains("`#% increased Spell Damage` : 10% / —"), "{md}");
    }
}
