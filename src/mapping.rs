//! Sessions de map de l'endgame, reconstituées depuis le log : entrée, retours par portail,
//! sous-zones (Abysse, temple d'Incursion…), morts, temps passé dedans hors AFK.
//!
//! Une map est reconnue à son code (« MapCrypt ») et à son seed : rentrer avec le même seed,
//! c'est revenir dans la même map par un portail ; un autre seed, c'est une nouvelle map.
//! Les heures sont celles du log (heure locale du joueur, comptée comme des secondes Unix).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::logwatch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunKind {
    /// Map (waystone), tour, logbook d'Expedition.
    Map,
    /// Arène d'un boss d'endgame (Roi dans les Brumes, Arbitre, Kulemak de l'Abysse…).
    Boss,
}

/// Ce qu'est une zone pour le suivi des maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Run(RunKind),
    /// Zone ouverte depuis une map, comptée avec elle (profondeurs de l'Abysse, temple…).
    Sub,
    /// Repaire ou ville : la map reste ouverte, le temps s'arrête.
    Safe,
    /// Zone de campagne : la map est terminée.
    Campaign,
    /// Le reste (Sanctum, hubs de ligue…) : comme une ville.
    Other,
}

pub fn place(code: &str) -> Place {
    if code.starts_with("MapUberBoss") || code.ends_with("Boss") || code.starts_with("Abyss_Pinnacle") || code.starts_with("Abyss_Boss") {
        return Place::Run(RunKind::Boss);
    }
    if code.contains("Hideout") || code == "G_Endgame_Town" {
        return Place::Safe;
    }
    if logwatch::act_of(code).is_some_and(|a| a <= 5) {
        return Place::Campaign;
    }
    if code.starts_with("Map") || code.starts_with("ExpeditionLogBook") {
        return Place::Run(RunKind::Map);
    }
    const SUB: [&str; 4] = ["Abyss_Depths", "IncursionTemple", "BreachDomain", "ChayulaLeague"];
    if SUB.iter().any(|s| code.starts_with(s)) || code.starts_with("ExpeditionSubArea") {
        return Place::Sub;
    }
    Place::Other
}

/// Waystone copié (Ctrl+C) avant d'ouvrir la map : ses chiffres d'en-tête et ses mods.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Waystone {
    pub name: String,
    pub tier: u32,
    /// En-tête : « Item Rarity: +33% », « Revives Available: 0 »…
    pub stats: Vec<String>,
    pub mods: Vec<String>,
    #[serde(default)]
    pub corrupted: bool,
}

/// Ligne « Clé: valeur » d'en-tête (pas un mod).
static STAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z][A-Za-z' ]+: [+-]?\d").unwrap());
static TIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Waystone Tier: (\d+)|\(Tier (\d+)\)").unwrap());
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+(?:\.\d+)?").unwrap());

impl Waystone {
    /// Texte copié en jeu, si c'est un waystone.
    pub fn parse(text: &str) -> Option<Waystone> {
        let it = crate::item::parse(text)?;
        if !it.item_class.starts_with("Waystone") {
            return None;
        }
        let tier = TIER.captures(&it.raw).and_then(|c| c.get(1).or(c.get(2))?.as_str().parse().ok()).unwrap_or(0);
        // En-tête : les lignes « Clé: valeur » entre le nom et le niveau d'objet.
        const SKIP: [&str; 6] = ["Item Class", "Rarity", "Waystone Tier", "Item Level", "Requires", "Note"];
        let head = it.raw.split("Item Level:").next().unwrap_or("");
        let stats = head
            .lines()
            .map(|l| l.trim().trim_end_matches(" (augmented)").to_string())
            .filter(|l| STAT.is_match(l) && !SKIP.iter().any(|k| l.starts_with(k)))
            .collect();
        let mods = it.mods.iter().filter(|m| !STAT.is_match(m) && !m.contains("Map Device")).cloned().collect();
        Some(Waystone { name: it.name.clone(), tier, stats, mods, corrupted: it.corrupted })
    }
}

/// Mod sans ses nombres, pour regrouper les maps qui l'avaient (« Monsters deal #% … »).
pub fn mod_pattern(line: &str) -> String {
    NUMBER.replace_all(line, "#").into_owned()
}

/// Une map (ou une arène de boss), du premier chargement au dernier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub code: String,
    /// Niveau de zone (65 = tier 1… 80 = tier 16 et plus).
    pub level: u32,
    pub seed: u64,
    pub kind: RunKind,
    /// Nom affiché par le jeu, dans sa langue.
    #[serde(default)]
    pub zone: Option<String>,
    /// Premier chargement et dernière sortie (heure du log).
    pub start: i64,
    pub last: i64,
    /// Temps passé dedans, sous-zones comprises, AFK compris (`afk_secs` à retirer).
    pub inside_secs: i64,
    #[serde(default)]
    pub afk_secs: i64,
    /// Entrées dans la map depuis la ville : 1 + retours par portail.
    pub entries: u32,
    pub deaths: u32,
    /// Sous-zones ouvertes depuis la map (« Abyss_Depths2 »…), sans doublon.
    #[serde(default)]
    pub sub_areas: Vec<String>,
    /// Waystone copié juste avant d'entrer.
    #[serde(default)]
    pub waystone: Option<Waystone>,
    /// En groupe : map d'un autre (on n'a pas lancé le device avant d'y entrer), ou un autre
    /// joueur y est entré ou en est sorti.
    #[serde(default)]
    pub group: bool,
}

impl Run {
    /// Temps de jeu dans la map, hors AFK.
    pub fn active_secs(&self) -> i64 {
        (self.inside_secs - self.afk_secs).max(0)
    }

    /// Allers-retours en ville (par portail ou après une mort).
    pub fn returns(&self) -> u32 {
        self.entries.saturating_sub(1)
    }
}

/// Un waystone copié sert à la première map ouverte dans cette demi-heure.
const WAYSTONE_SECS: i64 = 30 * 60;

/// Au-delà, le même code et le même seed sont une autre map (une instance ne dure pas).
const SAME_INSTANCE_SECS: i64 = 6 * 3600;

/// Les maps d'un personnage et son arbre d'Atlas. Seuls `runs`, `seen` et `atlas` sont
/// gardés : le reste se reconstruit en relisant la fin du log au démarrage.
#[derive(Default, Serialize, Deserialize)]
pub struct Book {
    pub runs: Vec<Run>,
    /// Passifs de l'Atlas posés : identifiant → nom affiché.
    #[serde(default)]
    pub atlas: BTreeMap<String, String>,
    /// Heure du dernier événement compté : ceux d'avant, relus au démarrage, ne recomptent
    /// rien et servent seulement à savoir où l'on est.
    #[serde(default)]
    pub seen: i64,
    #[serde(skip)]
    open: Option<usize>,
    #[serde(skip)]
    inside_since: Option<i64>,
    #[serde(skip)]
    afk_since: Option<i64>,
    /// En attente du nom affiché de la map qu'on vient d'ouvrir.
    #[serde(skip)]
    naming: bool,
    /// Device utilisé depuis la dernière map ouverte : la prochaine est à nous.
    #[serde(skip)]
    device: bool,
    /// Waystone copié, en attente de la prochaine map (et l'heure du log de la copie).
    #[serde(skip)]
    pending: Option<(Waystone, Option<i64>)>,
    #[serde(skip)]
    last_t: i64,
    /// Le dernier événement daté était nouveau.
    #[serde(skip)]
    last_fresh: bool,
    /// Changé depuis la dernière sauvegarde.
    #[serde(skip)]
    dirty: std::cell::Cell<bool>,
}

impl Book {
    pub fn load(path: &std::path::Path) -> Book {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Sauvegarde si quelque chose a changé.
    pub fn save(&self, path: &std::path::Path) {
        if !self.dirty.get() {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(path, serde_json::to_string(&self).unwrap_or_default()).is_ok() {
            self.dirty.set(false);
        }
    }

    /// Waystone copié en jeu : il ira à la prochaine map ouverte. `now` : heure du log.
    pub fn set_waystone(&mut self, w: Waystone, now: Option<i64>) {
        self.pending = Some((w, now));
    }

    /// Waystone en attente d'une map.
    pub fn pending_waystone(&self) -> Option<&Waystone> {
        self.pending.as_ref().map(|(w, _)| w)
    }

    /// Map en cours (ouverte, même depuis le repaire) et l'heure d'entrée si l'on est dedans.
    pub fn current(&self) -> Option<(&Run, Option<i64>)> {
        Some((self.runs.get(self.open?)?, self.inside_since))
    }

    /// Temps de jeu dans la map en cours à l'heure `now` (heure du log), hors AFK.
    pub fn live_secs(&self, now: i64) -> Option<i64> {
        let r = self.runs.get(self.open?)?;
        let running = self.inside_since.map(|s| (now - s).max(0) - self.afk_part(s, now)).unwrap_or(0);
        Some(r.active_secs() + running)
    }

    /// Maps de la séance en cours : les dernières, sans pause de plus de deux heures entre elles.
    pub fn sitting(&self) -> &[Run] {
        let mut first = self.runs.len();
        while first > 0 && (first == self.runs.len() || self.runs[first].start - self.runs[first - 1].last < 2 * 3600) {
            first -= 1;
        }
        &self.runs[first..]
    }

    /// Événement daté : vrai s'il est nouveau (pas déjà compté avant un redémarrage).
    fn fresh(&mut self, t: i64) -> bool {
        self.last_t = self.last_t.max(t);
        self.last_fresh = t > self.seen;
        if self.last_fresh {
            self.seen = t;
            self.dirty.set(true);
        }
        self.last_fresh
    }

    /// AFK compris dans [from, t], pour la map ouverte.
    fn afk_part(&self, from: i64, t: i64) -> i64 {
        self.afk_since.map(|s| (t - s.max(from)).max(0)).unwrap_or(0)
    }

    /// Sortie de la zone en cours : le temps passé dedans est ajouté à la map.
    fn leave(&mut self, t: i64, fresh: bool) {
        let (Some(i), Some(since)) = (self.open, self.inside_since.take()) else { return };
        if !fresh {
            return;
        }
        let afk = self.afk_part(since, t);
        let r = &mut self.runs[i];
        r.inside_secs += (t - since).max(0);
        r.afk_secs += afk;
        r.last = t;
        if self.afk_since.is_some() {
            self.afk_since = Some(t);
        }
    }

    pub fn area(&mut self, code: &str, level: u32, seed: u64, t: i64) {
        let fresh = self.fresh(t);
        let was_inside = self.inside_since.is_some();
        self.leave(t, fresh);
        self.naming = false;
        match place(code) {
            Place::Run(kind) => {
                let found = self.runs.iter().rposition(|r| r.code == code && r.seed == seed && t - r.last < SAME_INSTANCE_SECS);
                let i = match found {
                    Some(i) => {
                        // Revenir d'une sous-zone, ce n'est pas un retour par la ville.
                        if fresh && !was_inside {
                            self.runs[i].entries += 1;
                        }
                        i
                    }
                    None if fresh => {
                        // Waystone copié peu avant : c'est celui de cette map (pas d'une arène).
                        let waystone = match self.pending.take() {
                            Some((w, at)) if kind == RunKind::Map && at.is_none_or(|at| (0..WAYSTONE_SECS).contains(&(t - at))) => Some(w),
                            Some(p) if kind == RunKind::Boss => {
                                self.pending = Some(p);
                                None
                            }
                            _ => None,
                        };
                        self.runs.push(Run { code: code.to_string(), level, seed, kind, zone: None, start: t, last: t, inside_secs: 0, afk_secs: 0, entries: 1, deaths: 0, sub_areas: vec![], waystone, group: !self.device });
                        self.device = false;
                        self.runs.len() - 1
                    }
                    // Map d'avant le dernier comptage, jamais vue : rien à reprendre.
                    None => {
                        self.open = None;
                        return;
                    }
                };
                self.open = Some(i);
                self.inside_since = Some(t);
                self.naming = self.runs[i].zone.is_none();
            }
            Place::Sub => {
                let Some(i) = self.open.filter(|_| was_inside) else { return };
                if fresh && !self.runs[i].sub_areas.iter().any(|s| s == code) {
                    self.runs[i].sub_areas.push(code.to_string());
                }
                self.inside_since = Some(t);
            }
            Place::Safe | Place::Other => {}
            Place::Campaign => self.open = None,
        }
    }

    /// Passif de l'Atlas posé ou retiré (sans effet s'il l'était déjà : relire le log ne
    /// change rien).
    pub fn atlas_node(&mut self, id: &str, name: &str, allocated: bool) {
        let changed = if allocated { self.atlas.insert(id.to_string(), name.to_string()).is_none() } else { self.atlas.remove(id).is_some() };
        if changed {
            self.dirty.set(true);
        }
    }

    /// Nom affiché de la zone qui vient de charger.
    pub fn zone(&mut self, name: &str) {
        if !self.naming {
            return;
        }
        self.naming = false;
        if let Some(i) = self.open {
            self.runs[i].zone = Some(name.to_string());
            self.dirty.set(true);
        }
    }

    /// On vient de lancer une map au device.
    pub fn device(&mut self) {
        self.device = true;
    }

    /// Un autre joueur entre dans la zone ou en sort : si c'est la map, elle est en groupe.
    pub fn party(&mut self) {
        if let (true, Some(i), Some(_)) = (self.last_fresh, self.open, self.inside_since) {
            if !self.runs[i].group {
                self.runs[i].group = true;
                self.dirty.set(true);
            }
        }
    }

    /// Mort du personnage : comptée pour la map si elle a lieu dedans.
    pub fn death(&mut self, t: i64) {
        let fresh = self.fresh(t);
        if let (true, Some(i), Some(_)) = (fresh, self.open, self.inside_since) {
            self.runs[i].deaths += 1;
        }
    }

    pub fn afk(&mut self, on: bool, t: i64) {
        let fresh = self.fresh(t);
        if on {
            self.afk_since.get_or_insert(t);
            return;
        }
        let Some(s) = self.afk_since.take() else { return };
        if let (true, Some(i), Some(since)) = (fresh, self.open, self.inside_since) {
            self.runs[i].afk_secs += (t - s.max(since)).max(0);
        }
    }

    /// Le jeu a été relancé : le temps dans la map s'arrête au dernier événement vu.
    pub fn session(&mut self) {
        // Déjà compté si le dernier événement avait été lu avant le redémarrage de l'app.
        let fresh = self.last_fresh;
        self.leave(self.last_t, fresh);
        self.afk_since = None;
        self.naming = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const WAYSTONE: &str = "Item Class: Waystones
Rarity: Rare
Doom Trail
Waystone (Tier 15)
--------
Waystone Tier: 15
Revives Available: 0
Monster Pack Size: +12% (augmented)
Item Rarity: +33% (augmented)
Waystone Drop Chance: +140% (augmented)
--------
Item Level: 79
--------
{ Prefix Modifier \"Burning\" (Tier: 1) }
Area has patches of Burning Ground
{ Suffix Modifier \"of Fury\" (Tier: 2) }
Monsters deal 40(36-40)% of Damage as Extra Fire Damage
--------
Can be used in a Map Device, allowing you to enter a Map.
--------
Corrupted";

    #[test]
    fn waystone_text() {
        let w = Waystone::parse(WAYSTONE).unwrap();
        assert_eq!((w.name.as_str(), w.tier, w.corrupted), ("Doom Trail", 15, true));
        assert_eq!(w.stats, vec!["Revives Available: 0", "Monster Pack Size: +12%", "Item Rarity: +33%", "Waystone Drop Chance: +140%"]);
        assert_eq!(w.mods, vec!["Area has patches of Burning Ground", "Monsters deal 40% of Damage as Extra Fire Damage"]);
        assert_eq!(mod_pattern(&w.mods[1]), "Monsters deal #% of Damage as Extra Fire Damage");
        assert!(Waystone::parse(crate::item::tests::DUSK_VIGIL).is_none());
    }

    #[test]
    fn waystone_goes_to_the_next_map() {
        let w = Waystone::parse(WAYSTONE).unwrap();
        let mut b = Book::default();
        b.area("HideoutShrine", 65, 1, 1000);
        b.set_waystone(w.clone(), Some(1010));
        b.area("MapUberBoss_Ritual", 79, 3, 1020); // arène : le waystone attend
        b.area("HideoutShrine", 65, 1, 1100);
        b.area("MapCrypt", 79, 4, 1200);
        assert_eq!(b.runs[0].waystone, None);
        assert_eq!(b.runs[1].waystone.as_ref(), Some(&w));
        assert!(b.pending_waystone().is_none());
        // Copié trop longtemps avant : pas associé.
        b.set_waystone(w, Some(1300));
        b.area("MapPit", 79, 5, 1300 + 2 * 3600);
        assert_eq!(b.runs[2].waystone, None);
    }

    #[test]
    fn solo_or_group() {
        let mut b = Book::default();
        b.area("HideoutShrine", 65, 1, 1000);
        b.device();
        b.area("MapCrypt", 79, 2, 1010);
        b.area("HideoutShrine", 65, 1, 1100);
        // Rentrer par son propre portail : toujours solo.
        b.area("MapCrypt", 79, 2, 1200);
        assert!(!b.runs[0].group);
        // Un joueur arrive : groupe.
        b.party();
        assert!(b.runs[0].group);
        // Entrer sans avoir lancé le device : la map d'un autre.
        b.area("HideoutShrine", 65, 1, 1300);
        b.area("MapPit", 79, 3, 1400);
        assert!(b.runs[1].group);
        // Le device lancé longtemps avant compte encore (AFK entre les deux).
        b.area("HideoutShrine", 65, 1, 1500);
        b.device();
        b.area("MapMire", 79, 4, 1500 + 1800);
        assert!(!b.runs[2].group);
        // Un joueur croisé au repaire ne change rien.
        b.area("HideoutShrine", 65, 1, 3400);
        b.party();
        assert!(!b.runs[2].group);
    }

    #[test]
    fn atlas_tree() {
        let mut b = Book::default();
        b.atlas_node("AtlasRitualSmall1", "Augmentation de Tribut", true);
        b.atlas_node("AtlasRitualNotable3", "Grand tribut", true);
        b.atlas_node("AtlasRitualSmall1", "Augmentation de Tribut", false);
        b.atlas_node("AtlasRitualNotable3", "Grand tribut", true);
        assert_eq!(b.atlas.len(), 1);
    }

    #[test]
    fn places() {
        assert_eq!(place("MapCrypt"), Place::Run(RunKind::Map));
        assert_eq!(place("MapUberBoss_Ritual"), Place::Run(RunKind::Boss));
        assert_eq!(place("RitualLeagueBoss"), Place::Run(RunKind::Boss));
        assert_eq!(place("ExpeditionSubArea_OlrothBoss"), Place::Run(RunKind::Boss));
        // Repaire à l'image d'une arène, pas un combat.
        assert_eq!(place("HideoutArenaKulemak"), Place::Safe);
        assert_eq!(place("HideoutShrine"), Place::Safe);
        assert_eq!(place("MapHideoutShrine_Claimable"), Place::Safe);
        assert_eq!(place("G_Endgame_Town"), Place::Safe);
        assert_eq!(place("Abyss_Depths2"), Place::Sub);
        assert_eq!(place("IncursionTemplePresent"), Place::Sub);
        assert_eq!(place("P1_5"), Place::Campaign);
        assert_eq!(place("Sanctum_1_Foyer_1"), Place::Other);
        assert_eq!(place("ExpeditionLogBook_Peninsula"), Place::Run(RunKind::Map));
    }

    #[test]
    fn portals_deaths_and_sub_areas() {
        let mut b = Book::default();
        b.area("HideoutShrine", 65, 1, 1000);
        b.area("MapCrypt", 78, 42, 1010);
        b.zone("Crypte");
        b.death(1100);
        b.area("HideoutShrine", 65, 1, 1110); // mort : retour au repaire
        b.area("MapCrypt", 78, 42, 1150); // portail
        b.area("Abyss_Depths2", 79, 7, 1200);
        b.area("MapCrypt", 78, 42, 1300); // remontée de l'Abysse : pas un retour en ville
        b.area("HideoutShrine", 65, 1, 1400);
        let r = &b.runs[0];
        assert_eq!(b.runs.len(), 1);
        assert_eq!(r.zone.as_deref(), Some("Crypte"));
        assert_eq!((r.entries, r.returns(), r.deaths), (2, 1, 1));
        assert_eq!(r.inside_secs, 100 + 250);
        assert_eq!(r.sub_areas, vec!["Abyss_Depths2".to_string()]);
        // Nouveau seed : nouvelle map, la précédente reste dans l'historique.
        b.area("MapCrypt", 78, 43, 1500);
        assert_eq!(b.runs.len(), 2);
        assert_eq!(b.current().unwrap().0.seed, 43);
        // Retour en campagne : plus de map en cours.
        b.area("P1_5", 60, 9, 1600);
        assert!(b.current().is_none());
    }

    #[test]
    fn live_time_and_sitting() {
        let mut b = Book::default();
        b.area("MapPit", 80, 5, 1000);
        b.area("HideoutShrine", 65, 1, 1600);
        b.area("MapCrypt", 78, 6, 20_000);
        b.area("HideoutShrine", 65, 1, 20_300);
        b.area("MapCrypt", 78, 6, 20_400);
        assert_eq!(b.live_secs(20_450), Some(300 + 50));
        // Plus de deux heures entre la Pit et la Crypte : nouvelle séance.
        assert_eq!(b.sitting().len(), 1);
        b.area("HideoutShrine", 65, 1, 20_500);
        assert_eq!(b.live_secs(99_999), Some(400));
    }

    #[test]
    fn afk_is_not_play_time() {
        let mut b = Book::default();
        b.area("MapPit", 80, 5, 1000);
        b.afk(true, 1100);
        b.afk(false, 1160);
        b.afk(true, 1200);
        b.area("HideoutShrine", 65, 1, 1300); // toujours AFK en sortant
        b.afk(false, 1400);
        let r = &b.runs[0];
        assert_eq!((r.inside_secs, r.afk_secs, r.active_secs()), (300, 160, 140));
    }

    #[test]
    fn replayed_events_count_once() {
        let events = |b: &mut Book| {
            b.area("MapPit", 80, 5, 10);
            b.death(50);
            b.area("HideoutShrine", 65, 1, 60);
            b.area("MapPit", 80, 5, 70);
        };
        let mut b = Book::default();
        events(&mut b);
        let saved = serde_json::to_string(&b).unwrap();
        // Redémarrage de l'app : on relit la fin du log, puis la suite arrive.
        let mut b: Book = serde_json::from_str(&saved).unwrap();
        events(&mut b);
        b.area("HideoutShrine", 65, 1, 100);
        let r = &b.runs[0];
        assert_eq!(b.runs.len(), 1);
        assert_eq!((r.entries, r.deaths, r.inside_secs), (2, 1, 50 + 30));
    }

    #[test]
    fn sub_area_outside_a_map_is_ignored() {
        let mut b = Book::default();
        b.area("G3_2", 54, 3, 0);
        b.area("Abyss_Depths2", 54, 4, 10);
        b.area("G3_2", 54, 3, 20);
        assert!(b.runs.is_empty());
    }

    #[test]
    fn game_restart_stops_the_clock() {
        let mut b = Book::default();
        b.area("MapPit", 80, 5, 1000);
        b.death(1120);
        b.session();
        b.area("MapPit", 80, 5, 6000);
        b.area("HideoutShrine", 65, 1, 6010);
        assert_eq!(b.runs[0].inside_secs, 120 + 10);
    }
}
