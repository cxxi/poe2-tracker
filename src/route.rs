//! Route de leveling : contenus facultatifs de la campagne qu'il ne faut pas rater
//! (points de passif, résistances, esprit…). Données connues, embarquées dans le programme.
//!
//! Le jeu écrit chaque récompense permanente dans le log (« X has received +30 to Spirit »,
//! « You have received 2 Passive Skill Points »), ce qui permet de cocher tout seul ce qui est fait.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
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
    /// Textes anglais de `what` et `reward`.
    #[serde(default)]
    pub what_en: String,
    #[serde(default)]
    pub reward_en: String,
}

impl Reward {
    /// Ce qu'il faut faire, dans la langue de l'interface.
    pub fn what_text(&self) -> &str {
        if crate::i18n::english() && !self.what_en.is_empty() { &self.what_en } else { &self.what }
    }

    /// La récompense, dans la langue de l'interface.
    pub fn reward_text(&self) -> &str {
        if crate::i18n::english() && !self.reward_en.is_empty() { &self.reward_en } else { &self.reward }
    }
}

#[derive(Deserialize)]
struct Data {
    rewards: Vec<Reward>,
}

pub fn load() -> Vec<Reward> {
    serde_json::from_str::<Data>(include_str!("../data/route.json")).map(|d| d.rewards).unwrap_or_default()
}

/// Ce que les récompenses obtenues sur la route ajoutent au personnage, quand elles sont sûres :
/// résistances des boss (feu, froid, foudre) et emplacement de charme. Les récompenses au choix
/// (tatouages, bénédiction de Qimah) ne disent pas ce qui a été pris : non comptées.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bonuses {
    pub resistances: [f64; 3],
    pub charm_slots: usize,
    /// Effets des récompenses obtenues, au format des lignes du jeu (« +30 to Spirit »), pour la
    /// fiche de caractéristiques.
    pub lines: Vec<String>,
}

/// Récompenses fixes et leur effet, au format des lignes du jeu.
const FIXED: [(&str, &str); 9] = [
    ("a1_res_beira", "+10% to Cold Resistance"),
    ("a2_res_garukhan", "+10% to Lightning Resistance"),
    ("a3_res_blackjaw", "+10% to Fire Resistance"),
    ("a1_spirit_king_mists", "+30 to Spirit"),
    ("a3_spirit_ignagduk", "+30 to Spirit"),
    ("p3_spirit_lythara", "+40 to Spirit"),
    ("a1_life_candlemass", "+20 to maximum Life"),
    ("a4_mana_silent_hall", "5% increased maximum Mana"),
    ("p2_life_molten_gift", "5% increased maximum Life"),
];

static RES_PICK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(\d+)% to (Fire|Cold|Lightning|all Elemental) Resistances?").unwrap());

/// `picks` : ce que le joueur a reçu pour chaque récompense au choix (texte du journal) ; les
/// résistances choisies (tatouages, bénédiction) s'ajoutent à celles des boss.
pub fn bonuses(done: &BTreeSet<String>, picks: &BTreeMap<String, Vec<String>>) -> Bonuses {
    let mut b = Bonuses::default();
    for text in picks.values().flatten() {
        if let Some(c) = RES_PICK.captures(text) {
            let v: f64 = c[1].parse().unwrap_or(0.0);
            match &c[2] {
                "Fire" => b.resistances[0] += v,
                "Cold" => b.resistances[1] += v,
                "Lightning" => b.resistances[2] += v,
                _ => b.resistances.iter_mut().for_each(|r| *r += v),
            }
        }
    }
    for (id, element) in [("a3_res_blackjaw", 0), ("a1_res_beira", 1), ("a2_res_garukhan", 2)] {
        if done.contains(id) {
            b.resistances[element] += 10.0;
        }
    }
    if done.contains("a2_charm_relics") {
        b.charm_slots += 1;
    }
    b.lines = FIXED.iter().filter(|(id, _)| done.contains(*id)).map(|(_, l)| l.to_string()).chain(picks.values().flatten().cloned()).collect();
    b
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

/// Options d'une récompense modifiable (on peut retourner changer son choix) : libellé court
/// (bout de ligne de la route) et texte du jeu. Le journal annonce certains choix ; les
/// bénédictions de Qimah, jamais.
pub fn modifiable_options(id: &str) -> &'static [(&'static str, &'static str)] {
    match id {
        "a2_charm_relics" => &[(tkey!("Charges"), "30% increased Charm Charges gained"), (tkey!("Durée"), "30% increased Charm Effect Duration")],
        "a4_flask_goddess" => &[(tkey!("Vie"), "30% increased Life Recovery from Flasks"), (tkey!("Mana"), "30% increased Mana Recovery from Flasks")],
        "p2_boon_qimah" => &[
            (tkey!("Rés. élémentaires"), "+5% to all Elemental Resistances"),
            (tkey!("Attributs"), "+5 to all Attributes"),
            (tkey!("Défenses"), "15% increased Global Defences"),
            (tkey!("Recharge"), "12% increased Cooldown Recovery Rate"),
            (tkey!("Présence"), "20% increased Presence Area of Effect"),
            (tkey!("Vitesse"), "3% increased Movement Speed"),
            (tkey!("Expérience"), "5% increased Experience gain"),
        ],
        _ => &[],
    }
}

/// Nombre de choix que garde une récompense au choix : trois tatouages, un seul ailleurs (le
/// dernier reçu, une récompense modifiable pouvant être changée).
pub fn picks_allowed(id: &str) -> usize {
    if id == "a4_tattoos_halls" { 3 } else { 1 }
}

/// Récompense au choix que désigne une ligne du journal reçue dans sa zone : on en garde le
/// texte (ce qui a été pris). Les points de passif, d'ascendance et l'esprit n'en sont jamais.
pub fn match_pick<'a>(rewards: &'a [Reward], area: Option<&str>, text: &str) -> Option<&'a Reward> {
    let kind = kind_of(text);
    if matches!(kind, Some("passive" | "weaponset" | "ascendancy" | "spirit")) {
        return None;
    }
    let area = area?;
    rewards.iter().filter(|r| is_choice(r)).find(|r| {
        r.areas.iter().any(|x| x == area) && (r.kind == "other" || kind.is_some_and(|k| r.kind.split('|').any(|x| x == k)))
    })
}

/// Récompense laissée au choix du joueur (la ligne décrit les options).
pub fn is_choice(r: &Reward) -> bool {
    let t = r.reward.to_lowercase();
    r.kind.contains('|') || t.contains("choix") || t.contains(" ou ") || t.contains("modifiable")
}

/// Choix du build (Mobalytics) pour une récompense au choix : ceux dont la zone de quête est
/// une des zones de la ligne (« The Venom Crypts / Ziggurat Encampment »).
pub fn build_choices<'a>(r: &Reward, choices: &'a [crate::builds::QuestChoice]) -> Vec<&'a crate::builds::QuestChoice> {
    if !is_choice(r) {
        return vec![];
    }
    let zones: Vec<String> = r.zone.split('/').map(|z| z.trim().to_lowercase()).filter(|z| !z.is_empty()).collect();
    choices.iter().filter(|c| zones.iter().any(|z| *z == c.area.to_lowercase())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bonuses_from_done_rewards() {
        let done: BTreeSet<String> = ["a1_res_beira", "a2_charm_relics", "a4_tattoos_halls"].iter().map(|s| s.to_string()).collect();
        let b = bonuses(&done, &BTreeMap::new());
        assert_eq!((b.resistances, b.charm_slots, b.lines), ([0.0, 10.0, 0.0], 1, vec!["+10% to Cold Resistance".to_string()]));
        let picks = BTreeMap::from([("a4_tattoos_halls".to_string(), vec!["+5% to Fire Resistance".to_string(), "+5 to Intelligence".to_string()])]);
        let b = bonuses(&done, &picks);
        assert_eq!(b.resistances, [5.0, 10.0, 0.0]);
        assert_eq!(b.lines.len(), 3);
        let r = load();
        assert_eq!(match_pick(&r, Some("G4_4_2"), "+5% to Lightning Resistance").unwrap().id, "a4_tattoos_halls");
        assert_eq!(match_pick(&r, Some("G4_town"), "30% increased Global Armour, Evasion and Energy Shield").unwrap().id, "a4_shark_fin");
        assert!(match_pick(&r, Some("G4_town"), "2 Passive Skill Points").is_none());
        // Chaque id de la route utilisé ici existe bien dans les données.
        let ids: Vec<String> = load().into_iter().map(|r| r.id).collect();
        for id in ["a3_res_blackjaw", "a1_res_beira", "a2_res_garukhan", "a2_charm_relics"].into_iter().chain(FIXED.map(|(id, _)| id)) {
            assert!(ids.iter().any(|i| i == id), "{id}");
        }
    }

    #[test]
    fn build_choices_follow_the_quest_zone() {
        let r = load();
        let choice = |area: &str, reward: &str| crate::builds::QuestChoice { act: String::new(), area: area.into(), quest: String::new(), reward: reward.into() };
        let picks = [choice("The Venom Crypts", "25% increased Mana Regeneration Rate"), choice("Clearfell", "+10% to Cold Resistance"), choice("Halls of the Dead", "+5 to Intelligence"), choice("Halls of the Dead", "+5% to Fire Resistance")];
        let venom = r.iter().find(|x| x.id == "a3_venom_choice").unwrap();
        assert_eq!(build_choices(venom, &picks).len(), 1);
        assert_eq!(build_choices(r.iter().find(|x| x.id == "a4_tattoos_halls").unwrap(), &picks).len(), 2);
        // Pas de choix à Clearfell : rien à afficher.
        assert!(build_choices(r.iter().find(|x| x.id == "a1_res_beira").unwrap(), &picks).is_empty());
    }

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
            Reward { id: "a".into(), act: 1, areas: vec!["G1_11".into()], zone: String::new(), what: String::new(), reward: String::new(), kind: "passive".into(), importance: "must".into(), ..Default::default() },
            Reward { id: "b".into(), act: 1, areas: vec!["G1_town".into()], zone: String::new(), what: String::new(), reward: String::new(), kind: "passive".into(), importance: "must".into(), ..Default::default() },
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
