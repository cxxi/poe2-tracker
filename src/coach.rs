//! État de la partie (personnage, niveau, acte) et conseils déclenchés par le log.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::guide::{self, Catalog, Step};
use crate::logwatch::{self, Event};
use crate::mapping::Book;
use crate::route::{self, Reward};

#[derive(Clone, Serialize, Deserialize)]
pub struct CharState {
    pub class: String,
    pub level: u32,
    pub max_act: u32,
    pub endgame_step: u32,
    /// Dernière zone où ce personnage a été vu : le jeu l'y replace à la connexion.
    #[serde(default)]
    pub last_area: Option<String>,
    /// Ordre de dernière activité (plus grand = plus récent).
    #[serde(default)]
    pub last_seen: u64,
    /// Récompenses de la route de leveling déjà obtenues (identifiants).
    #[serde(default)]
    pub done: BTreeSet<String>,
    /// Ce qui a été pris pour chaque récompense au choix (texte du journal, balisage retiré).
    #[serde(default)]
    pub picks: BTreeMap<String, Vec<String>>,
    /// Ligue choisie pour ce personnage (le journal du jeu ne la donne pas).
    #[serde(default)]
    pub league: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    active: Option<String>,
    characters: BTreeMap<String, CharState>,
    #[serde(default)]
    seq: u64,
    /// Tout le log a déjà été parcouru une fois (recensement des personnages).
    #[serde(default)]
    pub full_scan: bool,
    /// Tout le log a été relu pour cocher les récompenses de la route déjà obtenues.
    #[serde(default)]
    route_scan: bool,
    /// Tout le log a été relu pour reconstituer les maps de l'endgame.
    #[serde(default)]
    maps_scan: bool,
    /// Tout le log a été relu pour retrouver ce qui a été pris aux récompenses au choix.
    #[serde(default)]
    picks_scan: bool,
    /// Tout le log a été relu pour l'arbre de l'Atlas (après les maps : une relecture de plus,
    /// sans rien recompter grâce au repère `seen` des maps).
    #[serde(default)]
    atlas_scan: bool,
    /// Joueurs vus rejoindre ou quitter la zone : des coéquipiers, jamais nos personnages.
    #[serde(default)]
    party: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedKind {
    Session,
    Step,
    Ascendancy,
    Level,
    Death,
    Whisper,
    Character,
    Reward,
}

#[derive(Clone, Debug)]
pub struct FeedLine {
    pub kind: FeedKind,
    pub head: String,
    pub details: Vec<String>,
}

fn line(kind: FeedKind, head: String, details: Vec<String>) -> FeedLine {
    FeedLine { kind, head, details }
}

pub struct Coach {
    pub steps: Vec<Step>,
    pub route: Vec<Reward>,
    catalog: Catalog,
    state: State,
    path: PathBuf,
    pub zone: Option<String>,
    pub area: Option<String>,
    last_act: Option<u32>,
    deaths_here: u32,
    /// Le personnage actif est confirmé par une ligne nommée (sinon deviné par la zone).
    pub confirmed: bool,
    /// Événements lus en direct (et non relus au démarrage).
    pub live: bool,
    /// Le jeu vient d'être lancé : la prochaine zone dira qui s'est connecté.
    awaiting_login: bool,
    /// Faux après une connexion non reconnue : on n'attribue ni zones ni progression
    /// au personnage précédent tant que le bon n'est pas identifié.
    attributing: bool,
    /// Maps de chaque personnage, chargées à la demande (characters/<nom>/maps.json).
    maps: BTreeMap<String, Book>,
    /// Secondes à ajouter à l'heure UTC pour avoir l'heure du log (chronomètre des maps).
    pub clock: Option<i64>,
}

impl Coach {
    pub fn new(steps: Vec<Step>, catalog: Catalog, path: PathBuf) -> Coach {
        let state = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Coach { steps, route: route::load(), catalog, state, path, zone: None, area: None, last_act: None, deaths_here: 0, live: false, confirmed: false, awaiting_login: false, attributing: true, maps: BTreeMap::new(), clock: None }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Position de l'étape courante dans la liste des étapes.
    pub fn current_step_index(&self) -> Option<usize> {
        let label = self.current_step()?.label.clone();
        self.steps.iter().position(|s| s.label == label)
    }

    pub fn level(&self) -> u32 {
        self.character().map(|(_, c)| c.level).unwrap_or(1)
    }

    pub fn full_scan_done(&self) -> bool {
        self.state.full_scan
    }

    pub fn mark_full_scan(&mut self) {
        self.state.full_scan = true;
    }

    pub fn route_scan_done(&self) -> bool {
        self.state.route_scan
    }

    pub fn mark_route_scan(&mut self) {
        self.state.route_scan = true;
    }

    /// Choix précisé à la main pour une récompense que le journal n'annonce pas.
    pub fn set_pick(&mut self, id: &str, text: &str) {
        let Some(name) = self.state.active.clone() else { return };
        if let Some(c) = self.state.characters.get_mut(&name) {
            c.picks.insert(id.to_string(), vec![text.to_string()]);
            self.save();
        }
    }

    pub fn picks_scan_done(&self) -> bool {
        self.state.picks_scan
    }

    pub fn mark_picks_scan(&mut self) {
        self.state.picks_scan = true;
    }

    /// Avant de relire tout le log : personne n'est actif et aucune dernière zone n'est connue,
    /// chaque session sera attribuée par les lignes qui nomment le personnage, comme au tout
    /// premier lancement (sinon le dernier personnage joué hériterait des maps de tous les
    /// autres, deviné d'après sa dernière zone).
    pub fn begin_replay(&mut self) {
        self.state.active = None;
        for c in self.state.characters.values_mut() {
            c.last_area = None;
        }
    }

    pub fn maps_scan_done(&self) -> bool {
        self.state.maps_scan && self.state.atlas_scan
    }

    pub fn mark_maps_scan(&mut self) {
        self.state.maps_scan = true;
        self.state.atlas_scan = true;
    }

    fn book_path(&self, name: &str) -> PathBuf {
        crate::paths::character_dir(self.path.parent().unwrap_or(std::path::Path::new(".")), name).join("maps.json")
    }

    /// Maps du personnage actif.
    pub fn maps(&mut self) -> Option<&Book> {
        let name = self.state.active.clone()?;
        Some(self.book(&name))
    }

    fn book(&mut self, name: &str) -> &mut Book {
        if !self.maps.contains_key(name) {
            let book = Book::load(&self.book_path(name));
            self.maps.insert(name.to_string(), book);
        }
        self.maps.get_mut(name).unwrap()
    }

    /// Waystone copié en jeu : il sera associé à la prochaine map du personnage actif.
    pub fn set_waystone(&mut self, w: crate::mapping::Waystone) -> bool {
        let Some(name) = self.state.active.clone() else { return false };
        let now = self.log_now();
        self.book(&name).set_waystone(w, now);
        true
    }

    /// Heure actuelle dans l'heure du log, si on sait la déduire.
    pub fn log_now(&self) -> Option<i64> {
        Some(logwatch::utc_now() + self.clock?)
    }

    /// Suivi des maps du personnage actif (une fois le personnage de l'événement connu).
    fn track_maps(&mut self, ev: &Event) {
        // Un événement reçu en direct donne l'heure du log de maintenant.
        if let (true, Event::Area { time, .. } | Event::Death { time, .. } | Event::Afk { time, .. }) = (self.live, ev) {
            self.clock = Some(logwatch::clock_offset(*time, logwatch::utc_now()));
        }
        if !self.attributing {
            return;
        }
        let Some(name) = self.state.active.clone() else { return };
        match ev {
            Event::Session => self.book(&name).session(),
            Event::Area { code, level, seed, time, .. } => self.book(&name).area(code, *level, *seed, *time),
            Event::Zone(zone) => self.book(&name).zone(zone),
            Event::Death { character, time } if *character == name => self.book(&name).death(*time),
            Event::Afk { on, time } => self.book(&name).afk(*on, *time),
            Event::Atlas { id, name: node, allocated } => self.book(&name).atlas_node(id, node, *allocated),
            Event::Device => self.book(&name).device(),
            Event::Party { .. } => self.book(&name).party(),
            _ => {}
        }
    }

    /// Pense-bête de la route : celles laissées en arrière, celles de l'acte en cours, puis
    /// celles de l'acte en cours déjà faites. Vide en endgame.
    /// Route jusqu'à l'acte en cours : ce qui reste à faire d'abord (actes passés, puis acte en
    /// cours), puis ce qui est fait, coché, de l'acte le plus récent au plus ancien.
    pub fn reminders(&self) -> (u32, Vec<(&Reward, bool)>) {
        let Some((_, c)) = self.character() else { return (0, vec![]) };
        let act = c.max_act;
        let (mut done, todo): (Vec<_>, Vec<_>) = self.route.iter().filter(|r| r.act <= act).map(|r| (r, c.done.contains(&r.id))).partition(|(_, d)| *d);
        done.sort_by_key(|(r, _)| std::cmp::Reverse(r.act));
        (act, todo.into_iter().chain(done).collect())
    }

    pub fn league(&self) -> Option<String> {
        self.character()?.1.league.clone()
    }

    pub fn set_league(&mut self, league: String) {
        let Some(name) = self.state.active.clone() else { return };
        if let Some(c) = self.state.characters.get_mut(&name) {
            c.league = Some(league);
            self.save();
        }
    }

    /// Coche ou décoche à la main (contenus que le log ne signale pas).
    pub fn toggle_done(&mut self, id: &str) {
        let Some(name) = self.state.active.clone() else { return };
        let Some(c) = self.state.characters.get_mut(&name) else { return };
        if !c.done.remove(id) {
            c.done.insert(id.to_string());
        }
        self.save();
    }

    /// Personnages connus : nom, classe, niveau, du plus récent au plus ancien.
    pub fn characters(&self) -> Vec<(String, String, u32)> {
        let mut list: Vec<(&String, &CharState)> = self.state.characters.iter().collect();
        list.sort_by(|a, b| b.1.last_seen.cmp(&a.1.last_seen));
        list.into_iter().map(|(n, c)| (n.clone(), c.class.clone(), c.level)).collect()
    }

    /// Choix manuel du personnage (correction d'une mauvaise détection).
    pub fn set_character(&mut self, name: &str) {
        if self.state.characters.contains_key(name) {
            self.state.active = Some(name.to_string());
            self.confirmed = true;
            self.attributing = true;
            self.touch();
            self.save();
        }
    }

    fn touch(&mut self) {
        self.state.seq += 1;
        let seq = self.state.seq;
        if !self.attributing {
            return;
        }
        if let Some(c) = self.state.active.clone().and_then(|n| self.state.characters.get_mut(&n)) {
            c.last_seen = seq;
            c.last_area = self.area.clone().or(c.last_area.take());
        }
    }

    /// Un personnage nommé dans le log devient le personnage actif (créé s'il est nouveau).
    /// Les coéquipiers (montées de niveau et morts aussi écrites dans le log) sont ignorés.
    fn identify(&mut self, name: &str, class: Option<&str>) -> Vec<FeedLine> {
        if self.state.party.contains(name) {
            return vec![];
        }
        let last_act = self.last_act.unwrap_or(1);
        let entry = self.state.characters.entry(name.to_string()).or_insert(CharState {
            class: class.unwrap_or("?").to_string(),
            level: 1,
            // Les zones vues avant cette ligne sont celles de ce personnage.
            max_act: last_act,
            endgame_step: 1,
            last_area: None,
            last_seen: 0,
            done: BTreeSet::new(),
            picks: BTreeMap::new(),
            league: None,
        });
        if let Some(cl) = class {
            entry.class = cl.to_string();
        }
        let changed = self.state.active.as_deref() != Some(name);
        let was_confirmed = self.confirmed;
        self.state.active = Some(name.to_string());
        self.confirmed = true;
        self.attributing = true;
        self.touch();
        if changed {
            vec![line(FeedKind::Character, tr!("Personnage : {name}", name), vec![])]
        } else if !was_confirmed {
            vec![line(FeedKind::Character, tr!("Personnage confirmé : {name}", name), vec![])]
        } else {
            vec![]
        }
    }

    /// Change de build : relit les étapes avec les noms de gemmes déjà chargés.
    pub fn reload_steps(&mut self, files: &[PathBuf]) {
        self.steps = guide::load_steps(files, &self.catalog);
    }

    /// Étape à utiliser pour juger l'équipement : celle de l'acte atteint, sinon la première.
    pub fn judging_step(&self) -> Option<&Step> {
        self.current_step().or(self.steps.first())
    }

    pub fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.path, serde_json::to_string_pretty(&self.state).unwrap_or_default());
        for (name, book) in &self.maps {
            book.save(&self.book_path(name));
        }
    }

    pub fn character(&self) -> Option<(&str, &CharState)> {
        let name = self.state.active.as_deref()?;
        Some((name, self.state.characters.get(name)?))
    }

    pub fn current_step(&self) -> Option<&Step> {
        let (_, c) = self.character()?;
        guide::step_for(&self.steps, c.max_act, c.endgame_step, c.level)
    }

    pub fn set_endgame_step(&mut self, n: u32) {
        if let Some(name) = self.state.active.clone() {
            if let Some(c) = self.state.characters.get_mut(&name) {
                c.endgame_step = n;
                self.save();
            }
        }
    }

    pub fn handle(&mut self, ev: &Event) -> Vec<FeedLine> {
        let out = self.react(ev);
        self.track_maps(ev);
        out
    }

    fn react(&mut self, ev: &Event) -> Vec<FeedLine> {
        match ev {
            Event::Session => {
                self.zone = None;
                self.area = None;
                self.last_act = None;
                self.deaths_here = 0;
                self.awaiting_login = true;
                self.confirmed = false;
                vec![line(FeedKind::Session, tr!("Le jeu vient d'être lancé.").into(), vec![])]
            }
            Event::Area { code, level, act, .. } => {
                if self.area.as_deref() != Some(code) {
                    self.deaths_here = 0;
                }
                self.area = Some(code.clone());
                let mut out = Vec::new();
                if self.awaiting_login {
                    // Connexion : le jeu replace le personnage dans sa dernière zone.
                    self.awaiting_login = false;
                    let guess = self
                        .state
                        .characters
                        .iter()
                        .filter(|(_, c)| c.last_area.as_deref() == Some(code))
                        .max_by_key(|(_, c)| c.last_seen)
                        .map(|(n, _)| n.clone());
                    self.attributing = guess.is_some();
                    match guess {
                        Some(name) => {
                            if self.state.active.as_deref() != Some(&name) {
                                out.push(line(FeedKind::Character, tr!("Personnage probable : {name} (à confirmer)", name), vec![]));
                            }
                            self.state.active = Some(name);
                        }
                        None => out.push(line(FeedKind::Character, tr!("Personnage non reconnu : il sera identifié au prochain niveau, récompense ou mort").into(), vec![])),
                    }
                }
                self.touch();
                let Some(act) = *act else { return out };
                self.last_act = Some(self.last_act.unwrap_or(0).max(act));
                if !self.attributing {
                    return out;
                }
                let Some(name) = self.state.active.clone() else { return out };
                let max_act = self.state.characters[&name].max_act;
                if act <= max_act {
                    return out;
                }
                let old = self.current_step().cloned();
                self.state.characters.get_mut(&name).unwrap().max_act = act;
                let Some(new) = self.current_step().cloned() else { return out };
                if old.as_ref().is_some_and(|o| o.label == new.label) {
                    return out;
                }
                let head = tr!("{step} atteinte (zone {code}, niveau {level})", step = guide::step_text(&new.label), code, level);
                out.push(line(FeedKind::Step, head, guide::diff_steps(old.as_ref(), &new, &self.catalog)));
                out
            }
            Event::Zone(name) => {
                if self.zone.as_deref() == Some(name) {
                    return vec![];
                }
                self.zone = Some(name.clone());
                if !(name.contains("Trial") || name.contains("Sekhemas")) {
                    return vec![];
                }
                let Some(step) = self.current_step() else { return vec![] };
                let index = self.steps.iter().position(|s| s.label == step.label).unwrap_or(0);
                let mut planned: Vec<String> = Vec::new();
                for s in self.steps.iter().skip(index).take(2) {
                    for n in guide::ascendancy_notables(s, &self.catalog) {
                        if !planned.contains(&n) {
                            planned.push(n);
                        }
                    }
                }
                if planned.is_empty() {
                    return vec![];
                }
                vec![line(FeedKind::Ascendancy, tr!("{name} : ordre prévu → {order}", name, order = planned.join(" > ")), vec![])]
            }
            Event::Level { character, .. } | Event::Death { character, .. } if self.state.party.contains(character) => vec![],
            Event::Received { character: Some(name), .. } if self.state.party.contains(name) => vec![],
            Event::Level { character, class, level } => {
                let mut out = self.identify(character, Some(class));
                let old_step = self.current_step().cloned();
                let c = self.state.characters.get_mut(character).unwrap();
                let old = c.level;
                c.level = *level;
                if *level <= old {
                    return out;
                }
                // Build découpé par tranches de niveaux : nouvelle étape en montant de niveau.
                if let Some(new) = self.current_step().cloned() {
                    if old_step.as_ref().is_some_and(|o| o.label != new.label) {
                        let head = tr!("{step} atteinte (niveau {level})", step = guide::step_text(&new.label), level);
                        out.push(line(FeedKind::Step, head, guide::diff_steps(old_step.as_ref(), &new, &self.catalog)));
                    }
                }
                let details = self
                    .current_step()
                    .map(|s| guide::new_at_level(s, old, *level).into_iter().map(|g| tr!("Le guide ({step}) ajoute : {g}", step = guide::step_text(&s.label), g)).collect())
                    .unwrap_or_default();
                out.push(line(FeedKind::Level, tr!("Niveau {level}", level), details));
                out
            }
            Event::Received { character, text } => {
                let mut out = character.as_deref().map(|n| self.identify(n, None)).unwrap_or_default();
                if !self.attributing {
                    return out;
                }
                let Some(name) = self.state.active.clone() else { return out };
                let area = self.area.clone();
                let act = area.as_deref().and_then(logwatch::act_of);
                let c = self.state.characters.get_mut(&name).unwrap();
                // Récompense au choix : on garde ce qui a été pris (même déjà cochée, pour
                // retrouver les choix en relisant le journal), et elle se coche.
                if let Some(r) = route::match_pick(&self.route, area.as_deref(), text) {
                    let got = crate::statdesc::strip_markup(text).trim_end_matches('.').to_string();
                    let list = c.picks.entry(r.id.clone()).or_default();
                    // L'emplacement de charme est la partie fixe des reliques, pas le choix.
                    if !list.contains(&got) && !got.contains("Charm Slot") {
                        list.push(got);
                        let keep = route::picks_allowed(&r.id);
                        if list.len() > keep {
                            list.drain(..list.len() - keep);
                        }
                    }
                    if c.done.insert(r.id.clone()) {
                        out.push(line(FeedKind::Reward, tr!("Fait : {reward} ({what})", reward = r.reward_text(), what = r.what_text()), vec![]));
                    }
                } else if let Some(r) = route::match_received(&self.route, &c.done, area.as_deref(), act, text, self.live) {
                    c.done.insert(r.id.clone());
                    out.push(line(FeedKind::Reward, tr!("Fait : {reward} ({what})", reward = r.reward_text(), what = r.what_text()), vec![]));
                }
                out
            }
            Event::Death { character, .. } => {
                // Seul un personnage déjà connu est identifié (pas un membre du groupe).
                let mut out = if self.state.characters.contains_key(character) { self.identify(character, None) } else { vec![] };
                if self.state.active.as_deref() != Some(character.as_str()) {
                    return out;
                }
                self.deaths_here += 1;
                let zone = self.zone.clone().unwrap_or_else(|| "?".into());
                let extra = if self.deaths_here > 1 { tr!(" ({n}e mort ici)", n = self.deaths_here) } else { String::new() };
                out.push(line(FeedKind::Death, tr!("Mort dans {zone}{extra}", zone, extra), vec![]));
                out
            }
            Event::Afk { .. } | Event::Atlas { .. } | Event::Device => vec![],
            Event::Whisper { from, text } => vec![line(FeedKind::Whisper, format!("{from} : {text}"), vec![])],
            Event::Party { character } => {
                if !self.state.party.insert(character.clone()) {
                    return vec![];
                }
                // Pris à tort pour un de nos personnages : on le retire, et si c'était l'actif,
                // on revient au dernier de nos personnages vus.
                self.state.characters.remove(character);
                if self.state.active.as_deref() == Some(character.as_str()) {
                    self.state.active = self.state.characters.iter().max_by_key(|(_, c)| c.last_seen).map(|(n, _)| n.clone());
                    self.confirmed = false;
                }
                vec![line(FeedKind::Character, tr!("Coéquipier : {character}", character), vec![])]
            }
        }
    }

    /// Résumé en une ligne pour l'interface.
    pub fn summary(&self) -> String {
        match self.character() {
            None => tr!("personnage pas encore détecté (il faut une montée de niveau dans le journal)").into(),
            // Le nom et l'étape sont déjà dans les listes du haut.
            Some((_, c)) => {
                let zone = self.zone.as_deref().unwrap_or(tr!("zone inconnue"));
                tr!("{class} · niveau {level} · {zone}", class = crate::gametext::name(&c.class), level = c.level, zone = crate::gametext::name(zone))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guide::Skill;

    fn coach() -> Coach {
        let skill = |n: &str, l| Skill { name: n.into(), from_level: l, supports: vec![], weapon_set: None };
        let steps = vec![
            Step { label: "Act 1".into(), skills: vec![skill("Contagion", 1), skill("Bonestorm", 15)], passives: vec![], targets: vec![], uniques: vec![], passive_sets: Default::default(), side_targets: vec![] },
            Step { label: "Act 2".into(), skills: vec![skill("Ice Nova", 1)], passives: vec![], targets: vec![], uniques: vec![], passive_sets: Default::default(), side_targets: vec![] },
        ];
        Coach::new(steps, Catalog::default(), PathBuf::from("/nonexistent/state.json"))
    }

    fn level(n: u32) -> Event {
        Event::Level { character: "Juliana".into(), class: "Sorceress".into(), level: n }
    }

    #[test]
    fn level_up_announces_guide_gem() {
        let mut c = coach();
        c.handle(&level(14));
        let out = c.handle(&level(15));
        assert!(out[0].details.iter().any(|d| d.contains("Bonestorm")), "{out:?}");
    }

    #[test]
    fn act_change_switches_step_and_never_goes_back() {
        let mut c = coach();
        c.handle(&level(16));
        let out = c.handle(&Event::Area { code: "G2_town".into(), level: 16, act: Some(2), seed: 1, time: 0 });
        assert_eq!(out[0].kind, FeedKind::Step);
        assert!(out[0].details.iter().any(|d| d.contains("Ice Nova")));
        assert!(c.handle(&Event::Area { code: "G1_town".into(), level: 15, act: Some(1), seed: 1, time: 0 }).is_empty());
        assert_eq!(c.current_step().unwrap().label, "Act 2");
    }

    #[test]
    fn deaths_counted_per_area() {
        let mut c = coach();
        c.handle(&level(5));
        c.handle(&Event::Area { code: "G1_9".into(), level: 9, act: Some(1), seed: 1, time: 0 });
        c.handle(&Event::Zone("Tomb".into()));
        c.handle(&Event::Death { character: "Juliana".into(), time: 0 });
        let out = c.handle(&Event::Death { character: "Juliana".into(), time: 0 });
        assert!(out.iter().any(|l| l.head.contains("2e mort ici")), "{out:?}");
        // La mort d'un inconnu (membre du groupe) n'est pas comptée.
        assert!(c.handle(&Event::Death { character: "Bob".into(), time: 0 }).is_empty());
    }

    #[test]
    fn party_member_never_becomes_active() {
        let mut c = coach();
        c.handle(&level(10));
        // Le coéquipier monte de niveau avant d'être reconnu : pris à tort pour un personnage…
        c.handle(&Event::Level { character: "Bob".into(), class: "Warrior".into(), level: 5 });
        assert_eq!(c.character().unwrap().0, "Bob");
        // … puis il rejoint la zone : retiré, on revient à Juliana.
        c.handle(&Event::Party { character: "Bob".into() });
        assert_eq!(c.character().unwrap().0, "Juliana");
        assert!(c.characters().iter().all(|(n, _, _)| n != "Bob"));
        c.handle(&Event::Level { character: "Bob".into(), class: "Warrior".into(), level: 6 });
        c.handle(&Event::Death { character: "Bob".into(), time: 0 });
        assert_eq!(c.character().unwrap().0, "Juliana");
    }

    /// Maps reconstituées depuis un vrai log, sur une copie de l'état :
    /// POE2_LOG=…/Client.txt POE2_STATE=…/state.json cargo test real_log_maps -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_log_maps() {
        let (Ok(log), Ok(state)) = (std::env::var("POE2_LOG"), std::env::var("POE2_STATE")) else { return };
        let mut c = Coach::new(vec![], Catalog::default(), PathBuf::from(state));
        c.begin_replay();
        let mut was = None;
        for ev in logwatch::read_all(std::path::Path::new(&log)) {
            c.handle(&ev);
            if c.state.active != was {
                if std::env::var("POE2_TRACE").is_ok() {
                    eprintln!("actif {:?} -> {:?} après {ev:?}", was, c.state.active);
                }
                was = c.state.active.clone();
            }
        }
        let names: Vec<String> = c.maps.keys().cloned().collect();
        for name in names {
            let runs = &c.book(&name).runs;
            let deaths: u32 = runs.iter().map(|r| r.deaths).sum();
            eprintln!("{name} : {} maps, {deaths} morts", runs.len());
            for r in runs.iter().rev().take(6) {
                eprintln!("    jour {} {:<24} niv {} {:?} {:>5.1} min (AFK {:.1}) entrées {} morts {} {:?} {:?}", r.start / 86_400, r.code, r.level, r.kind, r.active_secs() as f64 / 60.0, r.afk_secs as f64 / 60.0, r.entries, r.deaths, r.zone, r.sub_areas);
            }
        }
        c.save();
    }

    #[test]
    fn login_guesses_character_from_last_area_then_confirms() {
        let mut c = coach();
        c.handle(&Event::Session);
        c.handle(&Event::Area { code: "G2_3".into(), level: 20, act: Some(2), seed: 1, time: 0 });
        c.handle(&level(20)); // Juliana, vue en G2_3
        c.handle(&Event::Session);
        c.handle(&Event::Area { code: "G1_town".into(), level: 15, act: Some(1), seed: 1, time: 0 });
        c.handle(&Event::Level { character: "Bob".into(), class: "Monk".into(), level: 5 }); // Bob, vu en G1_town
        // Reconnexion : le jeu place le personnage en G2_3 → Juliana probable.
        c.handle(&Event::Session);
        let out = c.handle(&Event::Area { code: "G2_3".into(), level: 20, act: Some(2), seed: 1, time: 0 });
        assert!(out.iter().any(|l| l.head.contains("Juliana") && l.head.contains("à confirmer")), "{out:?}");
        assert_eq!(c.character().unwrap().0, "Juliana");
        assert!(!c.confirmed);
        let out = c.handle(&Event::Received { character: Some("Juliana".into()), text: "+30 to [Spirit|Spirit].".into() });
        assert!(c.confirmed && out[0].head.contains("confirmé"));
    }
}
