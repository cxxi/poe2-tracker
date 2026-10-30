//! Regex à coller dans la recherche d'un marchand en jeu : surligne les objets qui portent un
//! mod visé par le build à l'étape suivie.
//!
//! La recherche du jeu accepte une expression régulière entre guillemets, sans tenir compte de
//! la casse ; on y met un morceau court par mod, séparés par « | », les mods visés sur le plus
//! d'emplacements d'abord, jusqu'à la longueur que le jeu accepte.

use std::collections::BTreeMap;

use crate::guide::Step;

/// Longueur maximale du texte collé (guillemets compris).
pub const MAX_LEN: usize = 250;

/// Morceaux connus : (motif du modèle de stat, morceau de regex, nom lisible). Le premier motif
/// contenu dans le modèle l'emporte.
const KNOWN: &[(&str, &str, &str)] = &[
    ("to Level of all Spell Skills", "all spell sk", tkey!("niveaux de tous les sorts")),
    ("to Level of all Cold Spell", "cold spell sk", tkey!("niveaux des sorts de froid")),
    ("to Level of all Fire Spell", "fire spell sk", tkey!("niveaux des sorts de feu")),
    ("to Level of all Lightning Spell", "ning spell sk", tkey!("niveaux des sorts de foudre")),
    ("to Level of all Chaos Spell", "chaos spell sk", tkey!("niveaux des sorts de chaos")),
    ("to Level of all Physical Spell", "cal spell sk", tkey!("niveaux des sorts physiques")),
    ("to Level of all Minion", "all minion sk", tkey!("niveaux des sbires")),
    ("to Level of all Melee", "all melee sk", tkey!("niveaux de mêlée")),
    ("to Level of all Projectile", "projectile sk", tkey!("niveaux des projectiles")),
    ("increased Movement Speed", "movem", tkey!("vitesse de déplacement")),
    ("to maximum Life", "maximum li", tkey!("vie")),
    ("to maximum Mana", "maximum ma", tkey!("mana")),
    ("to maximum Energy Shield", "maximum en", tkey!("bouclier d'énergie")),
    ("to Spirit", "to spirit", tkey!("esprit")),
    ("to all Elemental Resistances", "all ele", tkey!("toutes résistances")),
    ("to Chaos Resistance", "chaos res", tkey!("résistance au chaos")),
    ("increased Spell Damage", "spell dam", tkey!("dégâts des sorts")),
    ("increased Cast Speed", "cast sp", tkey!("vitesse d'incantation")),
    ("increased Attack Speed", "attack sp", tkey!("vitesse d'attaque")),
    ("Critical Hit Chance for Spells", "e for spells", tkey!("critique des sorts")),
    ("Critical Spell Damage Bonus", "spell damage b", tkey!("dégâts critiques des sorts")),
    ("Critical Damage Bonus", "damage bonus", tkey!("dégâts critiques")),
    ("increased Critical Hit Chance", "crit.*chance$", tkey!("chance de critique")),
    ("Damage as Extra", "as extra", tkey!("dégâts en plus")),
    ("increased Physical Damage", "ed phys", tkey!("dégâts physiques")),
    ("increased Cold Damage", "ed cold d", tkey!("dégâts de froid")),
    ("increased Fire Damage", "ed fire d", tkey!("dégâts de feu")),
    ("increased Lightning Damage", "ed light", tkey!("dégâts de foudre")),
    ("increased Chaos Damage", "ed chaos d", tkey!("dégâts de chaos")),
    ("increased Elemental Damage", "ed elem", tkey!("dégâts élémentaires")),
    ("Cold Damage to Attacks", "cold damage to a", tkey!("froid ajouté aux attaques")),
    ("Fire Damage to Attacks", "fire damage to a", tkey!("feu ajouté aux attaques")),
    ("Lightning Damage to Attacks", "ning damage to a", tkey!("foudre ajoutée aux attaques")),
    ("Physical Damage to Attacks", "cal damage to a", tkey!("physique ajouté aux attaques")),
    ("increased Armour", "ed armour", tkey!("% d'armure")),
    ("increased Evasion", "ed evasion", tkey!("% d'évasion")),
    ("increased Energy Shield", "ed energy", tkey!("% de bouclier d'énergie")),
    ("to Armour", "to armour", tkey!("armure")),
    ("to Evasion Rating", "to evasion", tkey!("évasion")),
    ("Mana Regeneration Rate", "mana regen", tkey!("régénération de mana")),
    ("Life Regeneration", "life regen", tkey!("régénération de vie")),
    ("to Intelligence", "to int", tkey!("intelligence")),
    ("to Strength", "to str", tkey!("force")),
    ("to Dexterity", "to dex", tkey!("dextérité")),
    ("to all Attributes", "all attr", tkey!("tous les attributs")),
    ("Rarity of Items", "rarity", tkey!("rareté")),
];

/// Résistances élémentaires simples : regroupées en un seul morceau.
const RESISTANCES: [(&str, &str); 3] = [("to Fire Resistance", "fi"), ("to Cold Resistance", "co"), ("to Lightning Resistance", "li")];

/// Morceau de regex et nom lisible pour un modèle de stat (« #% increased Cast Speed »).
fn piece(template: &str) -> (String, String) {
    if let Some((_, re, name)) = KNOWN.iter().find(|(k, _, _)| template.contains(k)) {
        return (re.to_string(), crate::i18n::tr(name).to_string());
    }
    // Inconnu : les deux derniers mots du mod, sans les nombres.
    let words: Vec<&str> = template.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).collect();
    let tail = words[words.len().saturating_sub(2)..].join(" ").to_lowercase();
    let text: String = tail.chars().take(16).collect();
    (regex::escape(&text), template.replace('#', "…"))
}

/// Regex pour l'étape (guillemets compris) et ce qu'elle cherche, dans l'ordre. Vide si le build
/// ne vise aucun mod à cette étape.
pub fn build(step: &Step) -> (String, Vec<String>) {
    // Nombre d'emplacements qui visent chaque modèle, et sa première apparition (ordre stable).
    // Les résistances élémentaires simples comptent pour un seul mod.
    const RES: &str = "résistances";
    let mut wanted: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut elements: Vec<&str> = Vec::new();
    for (i, (_, _, targets)) in step.targets.iter().enumerate() {
        let mut seen: Vec<&str> = Vec::new();
        for t in targets {
            let key = match RESISTANCES.iter().find(|(k, _)| t.template.contains(k)) {
                Some((_, short)) => {
                    if !elements.contains(short) {
                        elements.push(short);
                    }
                    RES
                }
                None => t.template.as_str(),
            };
            if !seen.contains(&key) {
                seen.push(key);
                let e = wanted.entry(key.to_string()).or_insert((0, i));
                e.0 += 1;
            }
        }
    }
    let mut order: Vec<(&String, &(usize, usize))> = wanted.iter().collect();
    order.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.1.1.cmp(&b.1.1)));

    let mut pieces: Vec<(String, String)> = Vec::new();
    for (template, _) in order {
        let p = if template == RES {
            let re = if elements.len() == 1 { format!("{}.+res", elements[0]) } else { format!("({}).+res", elements.join("|")) };
            (re, tr!("résistances élémentaires").to_string())
        } else {
            piece(template)
        };
        if !pieces.iter().any(|(re, _)| *re == p.0) {
            pieces.push(p);
        }
    }
    // Aussi long que le jeu l'accepte, les plus demandés d'abord.
    let mut out = String::from("\"");
    let mut names = Vec::new();
    for (re, name) in pieces {
        let sep = if names.is_empty() { "" } else { "|" };
        if out.len() + sep.len() + re.len() + 1 > MAX_LEN {
            continue;
        }
        out.push_str(sep);
        out.push_str(&re);
        names.push(name);
    }
    out.push('"');
    if names.is_empty() {
        return (String::new(), names);
    }
    (out, names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gear::Slot;
    use crate::guide::Target;

    fn t(template: &str) -> Target {
        Target { template: template.into(), value: 10.0, line: String::new() }
    }

    fn step(targets: Vec<(Slot, Vec<Target>)>) -> Step {
        Step {
            label: "x".into(),
            skills: vec![],
            passives: vec![],
            targets: targets.into_iter().map(|(s, t)| (s, String::new(), t)).collect(),
            uniques: vec![],
            passive_sets: Default::default(), side_targets: vec![],
        }
    }

    #[test]
    fn most_wanted_first_and_resistances_grouped() {
        let s = step(vec![
            (Slot::Weapon, vec![t("#% increased Spell Damage"), t("+# to Level of all Cold Spell Skills")]),
            (Slot::Helmet, vec![t("# to maximum Life"), t("#% to Cold Resistance"), t("#% to Fire Resistance")]),
            (Slot::Boots, vec![t("# to maximum Life"), t("#% increased Movement Speed"), t("#% to Lightning Resistance")]),
        ]);
        let (re, names) = build(&s);
        assert_eq!(re, "\"maximum li|(co|fi|li).+res|spell dam|cold spell sk|movem\"", "{names:?}");
        assert_eq!(names[0], "vie");
        // Chaque morceau doit trouver le texte réel du mod, sans tenir compte de la casse.
        let r = regex::Regex::new(&format!("(?i){}", re.trim_matches('"'))).unwrap();
        for line in ["+48 to maximum Life", "+24% to Cold Resistance", "+2 to Level of all Cold Spell Skills", "30% increased Movement Speed"] {
            assert!(r.is_match(line), "{line}");
        }
        assert!(!r.is_match("+12 to Strength"));
    }

    #[test]
    fn stays_under_the_game_limit() {
        let many: Vec<Target> = (0..80).map(|i| t(&format!("#% increased Strange Effect Number{i}"))).collect();
        let (re, _) = build(&step(vec![(Slot::Body, many)]));
        assert!(re.len() <= MAX_LEN && re.starts_with('"') && re.ends_with('"'));
    }

    #[test]
    fn nothing_wanted_gives_nothing() {
        assert_eq!(build(&step(vec![])).0, "");
    }
}
