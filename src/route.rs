//! Route de leveling : contenus facultatifs de la campagne qu'il ne faut pas rater
//! (points de passif, résistances, esprit…). Données connues, embarquées dans le programme.
//!
//! Le jeu écrit chaque récompense permanente dans le log (« X has received +30 to Spirit »,
//! « You have received 2 Passive Skill Points »), ce qui permet de cocher tout seul ce qui est fait.

use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Reward {
    pub id: String,
    pub act: u32,
    /// Codes des zones où la récompense tombe (zone du boss, ou ville si elle est remise par un PNJ).
    pub areas: Vec<String>,
    pub zone: String,
    pub what: String,
    pub reward: String,
    /// Nature(s) annoncée(s) par le log : passive, resist, spirit, life, mana, attribute,
    /// ascendancy ; `other` = rien dans le log, à cocher à la main.
    pub kind: String,
    /// `must`, `recommended` ou `story` (obligatoire de toute façon).
    pub importance: String,
}

#[derive(Deserialize)]
struct Data {
    rewards: Vec<Reward>,
}

pub fn load() -> Vec<Reward> {
    serde_json::from_str::<Data>(include_str!("../data/route.json")).map(|d| d.rewards).unwrap_or_default()
}

/// Nature d'une récompense d'après le texte du log (`None` : rien à cocher).
pub fn kind_of(text: &str) -> Option<&'static str> {
    if text.contains("Genesis Tree") {
        return None; // arbre de l'endgame, hors campagne
    }
    if text.contains("Weapon Set Passive") {
        Some("weaponset")
    } else if text.contains("Passive Skill Point") {
        Some("passive")
    } else if text.contains("Resistance") {
        Some("resist")
    } else if text.contains("Spirit") {
        Some("spirit")
    } else if text.contains("Life") {
        Some("life")
    } else if text.contains("Mana") {
        Some("mana")
    } else if ["Strength", "Dexterity", "Intelligence"].iter().any(|a| text.contains(a)) {
        Some("attribute")
    } else if text.contains("Ascendancy") {
        Some("ascendancy")
    } else {
        None
    }
}

/// Récompense que désigne une ligne du log : même nature, dans la zone où l'on est,
/// sinon (`fallback`, en direct seulement) la première pas encore faite de cette nature dans
/// l'acte. Sans repli, relire une session déjà vue ne coche jamais rien de plus.
pub fn match_received<'a>(rewards: &'a [Reward], done: &BTreeSet<String>, area: Option<&str>, act: Option<u32>, text: &str, fallback: bool) -> Option<&'a Reward> {
    let kind = kind_of(text)?;
    // Une récompense au choix accepte plusieurs natures : « resist|attribute ».
    let open = |r: &&Reward| r.kind.split('|').any(|k| k == kind) && !done.contains(&r.id);
    rewards
        .iter()
        .filter(open)
        .find(|r| area.is_some_and(|a| r.areas.iter().any(|x| x == a)))
        .or_else(|| rewards.iter().filter(open).find(|r| fallback && Some(r.act) == act))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_valid() {
        let r = load();
        assert!(!r.is_empty());
        assert!(r.iter().all(|x| ["must", "recommended", "story"].contains(&x.importance.as_str())), "importance inconnue");
        let ids: BTreeSet<&str> = r.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids.len(), r.len(), "identifiants en double");
    }

    #[test]
    fn received_lines_are_matched_by_kind_and_area() {
        let r = vec![
            Reward { id: "a".into(), act: 1, areas: vec!["G1_11".into()], zone: String::new(), what: String::new(), reward: String::new(), kind: "passive".into(), importance: "must".into() },
            Reward { id: "b".into(), act: 1, areas: vec!["G1_town".into()], zone: String::new(), what: String::new(), reward: String::new(), kind: "passive".into(), importance: "must".into() },
        ];
        let mut done = BTreeSet::new();
        let text = "2 Passive Skill Points.";
        assert_eq!(match_received(&r, &done, Some("G1_town"), Some(1), text, false).unwrap().id, "b");
        done.insert("b".to_string());
        // Session relue : la récompense déjà cochée n'en coche pas une autre.
        assert!(match_received(&r, &done, Some("G1_town"), Some(1), text, false).is_none());
        // En direct, zone inattendue : première de l'acte pas encore faite.
        assert_eq!(match_received(&r, &done, Some("G1_5"), Some(1), text, true).unwrap().id, "a");
        assert!(match_received(&r, &done, None, Some(1), "2 Genesis Tree Ring Skill Points.", true).is_none());
        assert!(match_received(&r, &done, None, Some(1), "2 Weapon Set Passive Skill Points.", true).is_none());
    }
}
