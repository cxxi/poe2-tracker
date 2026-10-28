//! Poids des mods sur le marché, réglés à la main : ce qu'une ligne pèse dans le prix d'un
//! objet, selon le type de pièce. La note d'un objet est la somme, pour chaque mod, de son
//! poids × sa valeur rapportée à une valeur de référence (un bon tirage de fin de jeu), plafonnée
//! à 1,5. Deux objets de même note se valent à peu près, quels que soient leurs mods.

use crate::trade::template;

/// Une ligne de la table : motif contenu dans le modèle du mod (« #% increased Movement
/// Speed »), valeur de référence, poids, et catégories de pièce concernées (préfixes de
/// `trade::category` ; vide = toutes). La première ligne qui correspond l'emporte.
struct Entry {
    pattern: &'static str,
    reference: f64,
    weight: f64,
    only: &'static [&'static str],
}

const fn e(pattern: &'static str, reference: f64, weight: f64) -> Entry {
    Entry { pattern, reference, weight, only: &[] }
}

const fn on(pattern: &'static str, reference: f64, weight: f64, only: &'static [&'static str]) -> Entry {
    Entry { pattern, reference, weight, only }
}

const WEAPON: &[&str] = &["weapon"];
const CASTER: &[&str] = &["weapon.wand", "weapon.staff", "weapon.sceptre", "armour.focus"];

const TABLE: &[Entry] = &[
    // Niveaux de gemmes : ce qui se vend le plus cher.
    e("Level of all Skills", 3.0, 12.0),
    e("Level of all Spell Skills", 3.0, 10.0),
    e("Spell Skills", 4.0, 8.0),
    e("Level of all Minion Skills", 4.0, 8.0),
    e("Level of all Melee Skills", 4.0, 8.0),
    e("Level of all Projectile Skills", 4.0, 8.0),
    // Vitesse.
    e("increased Movement Speed", 35.0, 10.0),
    e("increased Cast Speed", 30.0, 5.0),
    e("increased Attack Speed", 25.0, 5.0),
    e("increased Skill Speed", 15.0, 4.0),
    // Dégâts.
    e("Damage as Extra", 50.0, 7.0),
    on("increased Spell Damage", 120.0, 6.0, CASTER),
    e("increased Spell Damage", 35.0, 3.0),
    on("increased Physical Damage", 170.0, 7.0, WEAPON),
    on("Adds # to #", 40.0, 5.0, WEAPON),
    e("Adds # to #", 20.0, 2.5),
    e("Critical Damage Bonus", 40.0, 4.0),
    e("Critical Hit Chance", 40.0, 4.0),
    e("increased Cold Damage", 80.0, 3.0),
    e("increased Fire Damage", 80.0, 3.0),
    e("increased Lightning Damage", 80.0, 3.0),
    e("increased Chaos Damage", 80.0, 3.0),
    e("increased Elemental Damage", 80.0, 3.0),
    e("Penetrat", 20.0, 3.0),
    // Vie, mana, esprit.
    e("to Spirit", 60.0, 5.0),
    e("to maximum Life", 200.0, 4.0),
    e("increased maximum Life", 10.0, 4.0),
    e("to maximum Mana", 150.0, 1.5),
    e("increased maximum Mana", 10.0, 1.5),
    e("Life Leech", 10.0, 1.5),
    e("Mana Leech", 10.0, 1.0),
    e("Life Regeneration", 20.0, 1.0),
    e("Mana Regeneration Rate", 50.0, 1.0),
    // Résistances.
    e("to all Elemental Resistances", 18.0, 5.0),
    e("to Chaos Resistance", 27.0, 3.5),
    e("Resistances", 30.0, 4.0),
    e("Resistance", 45.0, 3.0),
    // Défenses.
    e("to maximum Energy Shield", 100.0, 3.0),
    e("increased Energy Shield", 100.0, 2.5),
    e("increased Armour", 100.0, 2.0),
    e("increased Evasion", 100.0, 2.0),
    e("to Armour", 150.0, 1.5),
    e("to Evasion Rating", 150.0, 1.5),
    e("Block", 20.0, 3.0),
    // Le reste.
    e("to all Attributes", 15.0, 1.5),
    e("to Strength", 30.0, 1.0),
    e("to Dexterity", 30.0, 1.0),
    e("to Intelligence", 30.0, 1.0),
    e("Rarity of Items", 30.0, 1.5),
    on("to Accuracy Rating", 300.0, 2.0, WEAPON),
    e("to Accuracy Rating", 300.0, 1.0),
    e("Projectile Speed", 40.0, 1.0),
    e("Stun Threshold", 150.0, 0.2),
    e("Thorns", 50.0, 0.5),
    e("Light Radius", 30.0, 0.0),
    e("Attribute Requirements", 30.0, 0.2),
    e("Charm", 30.0, 0.5),
    e("Flask", 30.0, 0.5),
];

/// Poids d'un mod sans ligne dans la table, compté comme un tirage moyen.
const UNKNOWN_WEIGHT: f64 = 1.0;
/// Un tirage au-delà de 1,5 fois la référence ne compte pas plus.
const QUALITY_MAX: f64 = 1.5;

/// Poids et valeur de référence du mod de modèle `template` sur une pièce de catégorie
/// `category` (« armour.boots »…).
fn entry(category: &str, template: &str) -> Option<&'static Entry> {
    TABLE.iter().find(|e| template.contains(e.pattern) && (e.only.is_empty() || e.only.iter().any(|c| category.starts_with(c))))
}

/// Ce que pèse une ligne de mod (« +24 to Dexterity ») dans la note de l'objet.
pub fn contribution(category: &str, line: &str) -> f64 {
    let (tpl, value) = template(line);
    match (entry(category, &tpl), value) {
        (Some(e), Some(v)) if e.reference > 0.0 => e.weight * (v / e.reference).min(QUALITY_MAX),
        (Some(e), _) => e.weight,
        (None, _) => UNKNOWN_WEIGHT,
    }
}

/// Note d'un objet : somme des contributions de ses mods.
pub fn score(category: &str, lines: &[String]) -> f64 {
    lines.iter().map(|l| contribution(category, l)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_and_light_lines() {
        let speed = contribution("armour.boots", "25% increased Movement Speed");
        let stun = contribution("armour.boots", "+152 to Stun Threshold");
        let res = contribution("armour.boots", "+40% to Lightning Resistance");
        assert!(speed > 6.0 && stun < 0.3 && res > 2.5, "{speed} {stun} {res}");
        // Une ligne de fin de jeu ne compte pas plus que 1,5 fois la référence.
        assert_eq!(contribution("armour.boots", "+999% to Fire Resistance"), 3.0 * 1.5);
        // Dégâts des sorts : référence d'arme sur un bâton, de bijou ailleurs.
        assert!((contribution("weapon.staff", "120% increased Spell Damage") - 6.0).abs() < 1e-9);
        assert!((contribution("accessory.amulet", "35% increased Spell Damage") - 3.0).abs() < 1e-9);
    }

    #[test]
    fn scores_add_up() {
        let lines = vec!["25% increased Movement Speed".to_string(), "+24 to Dexterity".to_string()];
        let s = score("armour.boots", &lines);
        assert!((s - (10.0 * 25.0 / 35.0 + 1.0 * 24.0 / 30.0)).abs() < 1e-9);
    }
}
