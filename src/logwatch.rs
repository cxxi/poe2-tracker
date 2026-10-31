//! Lecture de Client.txt, le log du client : zones, niveaux, morts, messages privés.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::mpsc::Sender;
use std::time::Duration;

use eframe::egui;
use regex::Regex;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Le jeu vient d'être lancé (nouvelle session dans le log).
    Session,
    /// Génération d'une zone : code ("G2_4"), niveau de zone, seed (le même pour une
    /// instance rouverte) et heure du log.
    Area { code: String, level: u32, act: Option<u32>, seed: u64, time: i64 },
    /// Nom affiché de la zone.
    Zone(String),
    Level { character: String, class: String, level: u32 },
    Death { character: String, time: i64 },
    /// Mode absent (AFK) activé ou désactivé.
    Afk { on: bool, time: i64 },
    /// Passif de l'Atlas posé ou retiré : identifiant (« AtlasRitualNotable3 ») et nom affiché.
    Atlas { id: String, name: String, allocated: bool },
    /// Talent du personnage pris ou retiré (« spells18 ») ; la ligne ne nomme pas le personnage.
    Passive { id: String, allocated: bool },
    /// Récompense de quête : « X has received +30 to Spirit » (nommée) ou
    /// « You have received 2 Passive Skill Points » (sans nom).
    Received { character: Option<String>, text: String },
    Whisper { from: String, text: String },
    /// « X has joined the area » / « has left the area » : un autre joueur (groupe), jamais soi.
    Party { character: String },
    /// Fermeture de l'Atlas (« Atlas input suspension DISABLED ») : on vient d'utiliser le
    /// device, pour lancer une map à soi.
    Device,
}

static LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2}) \d+ \w+ \[\w+ Client \d+\] (.*)$").unwrap());
static AREA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^Generating level (\d+) area "([^"]+)"(?: with seed (\d+))?"#).unwrap());
static SCENE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[SCENE\] Set Source \[(.+)\]$").unwrap());
static LEVEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^: (.+) \((\w+)\) is now level (\d+)$").unwrap());
static SLAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^: (.+) (?:has been slain|vient de mourir)\.$").unwrap());
static PASSIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Successfully (un)?allocated passive skill id: (\w+), name: (.*)$").unwrap());
static AFK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^: (?:AFK mode is now (ON|OFF)|Le mode Absent \(AFK\) est (désormais activé|désactivé))").unwrap());
static RECEIVED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^: (?:(\S+) has|You have) received (.+)$").unwrap());
static PARTY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^: (\S+) (?:has (?:joined|left) the area|a (?:rejoint|quitté) la zone)\.$").unwrap());
static WHISPER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^@From (.+?): (.*)$").unwrap());
static ACT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^G(\d)_").unwrap());
static INTERLUDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^P\d_").unwrap());
const SESSION_START: &str = "***** LOG FILE OPENING *****";

/// Acte de campagne d'après le code de zone : G1–G4 = actes, P1–P3 = interludes
/// (« acte 5 » des guides), cartes et ville de l'endgame = 6. `None` pour les zones qui ne
/// disent rien de la progression (repaires réclamables dès la campagne, Sanctum…).
pub fn act_of(code: &str) -> Option<u32> {
    if let Some(c) = ACT.captures(code) {
        return c[1].parse().ok();
    }
    if INTERLUDE.is_match(code) {
        return Some(5);
    }
    if code.contains("Hideout") {
        return None;
    }
    (code.starts_with("Map") || code.starts_with("G_Endgame")).then_some(6)
}

pub fn parse_line(raw: &str) -> Option<Event> {
    let raw = raw.trim_end();
    if raw.ends_with(SESSION_START) {
        return Some(Event::Session);
    }
    let line = LINE.captures(raw)?;
    // Heure locale du joueur, comptée comme une date UTC : ne sert qu'à mesurer des durées.
    let time = crate::trade::unix_time(&line[1]).unwrap_or(0);
    let msg = line.get(2)?.as_str();
    if let Some(c) = AREA.captures(msg) {
        let seed = c.get(3).and_then(|s| s.as_str().parse().ok()).unwrap_or(0);
        return Some(Event::Area { code: c[2].to_string(), level: c[1].parse().ok()?, act: act_of(&c[2]), seed, time });
    }
    if let Some(c) = SCENE.captures(msg) {
        return (!matches!(&c[1], "(null)" | "(unknown)")).then(|| Event::Zone(c[1].to_string()));
    }
    if let Some(c) = LEVEL.captures(msg) {
        return Some(Event::Level { character: c[1].to_string(), class: c[2].to_string(), level: c[3].parse().ok()? });
    }
    if let Some(c) = SLAIN.captures(msg) {
        return Some(Event::Death { character: c[1].to_string(), time });
    }
    if let Some(c) = PASSIVE.captures(msg) {
        let (id, allocated) = (c[2].to_string(), c.get(1).is_none());
        if id.starts_with("Atlas") {
            return Some(Event::Atlas { id, name: c[3].trim().to_string(), allocated });
        }
        return Some(Event::Passive { id, allocated });
    }
    if let Some(c) = AFK.captures(msg) {
        return Some(Event::Afk { on: c.get(1).map_or(c.get(2).is_some_and(|m| m.as_str().starts_with("désormais")), |m| m.as_str() == "ON"), time });
    }
    if let Some(c) = RECEIVED.captures(msg) {
        return Some(Event::Received { character: c.get(1).map(|m| m.as_str().to_string()), text: c[2].to_string() });
    }
    if let Some(c) = PARTY.captures(msg) {
        return Some(Event::Party { character: c[1].to_string() });
    }
    if msg == "Atlas input suspension DISABLED" {
        return Some(Event::Device);
    }
    if let Some(c) = WHISPER.captures(msg) {
        return Some(Event::Whisper { from: c[1].to_string(), text: c[2].to_string() });
    }
    None
}

/// Événements de la dernière session présente dans les derniers octets du log.
pub fn read_recent(path: &Path, max_bytes: u64) -> Vec<Event> {
    let Ok(mut f) = File::open(path) else { return vec![] };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(size.saturating_sub(max_bytes)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if size > max_bytes && !lines.is_empty() {
        lines.remove(0); // première ligne probablement coupée
    }
    let start = lines.iter().rposition(|l| l.trim_end().ends_with(SESSION_START)).unwrap_or(0);
    lines[start..].iter().filter_map(|l| parse_line(l)).collect()
}

/// Secondes à ajouter à l'heure UTC pour obtenir l'heure du log (heure locale du joueur),
/// au quart d'heure près.
pub fn clock_offset(log_time: i64, utc: i64) -> i64 {
    ((log_time - utc) as f64 / 900.0).round() as i64 * 900
}

pub fn utc_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Décalage de l'heure du log d'après sa dernière ligne et sa date de modification.
pub fn log_clock_offset(path: &Path) -> Option<i64> {
    let mut f = File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let _ = f.seek(SeekFrom::Start(meta.len().saturating_sub(64 * 1024)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let last = text.lines().rev().find_map(|l| LINE.captures(l).and_then(|c| crate::trade::unix_time(&c[1])))?;
    Some(clock_offset(last, modified))
}

/// Tous les événements du log, du début à la fin (recensement des personnages).
pub fn read_all(path: &Path) -> Vec<Event> {
    let Ok(data) = std::fs::read(path) else { return vec![] };
    String::from_utf8_lossy(&data).lines().filter_map(parse_line).collect()
}

/// Suit le log comme `tail -f` dans un fil dédié (gère la recréation au relancement du jeu).
pub fn follow(path: PathBuf, tx: Sender<Event>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let mut pos = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut partial = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(300));
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.len() < pos {
                pos = 0; // fichier recréé
                partial.clear();
            }
            if meta.len() == pos {
                continue;
            }
            let Ok(mut f) = File::open(&path) else { continue };
            if f.seek(SeekFrom::Start(pos)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(f);
            let mut bytes = Vec::new();
            let mut sent = false;
            // Lecture en octets : une ligne en UTF-8 invalide ne doit pas bloquer le suivi.
            while reader.read_until(b'\n', &mut bytes).unwrap_or(0) > 0 {
                pos += bytes.len() as u64;
                let line = String::from_utf8_lossy(&bytes).into_owned();
                bytes.clear();
                if line.ends_with('\n') {
                    partial.push_str(&line);
                    if let Some(ev) = parse_line(&partial) {
                        if tx.send(ev).is_err() {
                            return;
                        }
                        sent = true;
                    }
                    partial.clear();
                } else {
                    partial.push_str(&line);
                }
            }
            if sent {
                ctx.request_repaint();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "2026/10/24 05:28:26 322853657 3ef23348 [INFO Client 312] ";

    #[test]
    fn parses_events() {
        assert_eq!(
            parse_line(&format!("{P}: Juliana_novachrono (Sorceress) is now level 11")),
            Some(Event::Level { character: "Juliana_novachrono".into(), class: "Sorceress".into(), level: 11 })
        );
        assert_eq!(
            parse_line(&format!("{P}Generating level 16 area \"G2_town\" with seed 1")),
            Some(Event::Area { code: "G2_town".into(), level: 16, act: Some(2), seed: 1, time: 1792819706 })
        );
        assert_eq!(parse_line(&format!("{P}[SCENE] Set Source [Clearfell]")), Some(Event::Zone("Clearfell".into())));
        assert_eq!(parse_line(&format!("{P}[SCENE] Set Source [(null)]")), None);
        assert_eq!(parse_line(&format!("{P}: Juliana has been slain.")), Some(Event::Death { character: "Juliana".into(), time: 1792819706 }));
        assert_eq!(parse_line(&format!("{P}: Cxxisag vient de mourir.")), Some(Event::Death { character: "Cxxisag".into(), time: 1792819706 }));
        assert_eq!(parse_line(&format!("{P}: AFK mode is now ON. Autoreply \"This player is AFK.\"")), Some(Event::Afk { on: true, time: 1792819706 }));
        assert_eq!(parse_line(&format!("{P}: Le mode Absent (AFK) est désactivé.")), Some(Event::Afk { on: false, time: 1792819706 }));
        assert_eq!(
            parse_line(&format!("{P}Successfully allocated passive skill id: AtlasRitualSmall1, name: Augmentation de Tribut")),
            Some(Event::Atlas { id: "AtlasRitualSmall1".into(), name: "Augmentation de Tribut".into(), allocated: true })
        );
        assert_eq!(
            parse_line(&format!("{P}Successfully unallocated passive skill id: AtlasAbyssSmall14_, name: Depths Chance")),
            Some(Event::Atlas { id: "AtlasAbyssSmall14_".into(), name: "Depths Chance".into(), allocated: false })
        );
        // Talents du personnage, à part de l'Atlas.
        assert_eq!(
            parse_line(&format!("{P}Successfully allocated passive skill id: energy_shield35, name: Fusion")),
            Some(Event::Passive { id: "energy_shield35".into(), allocated: true })
        );
        assert_eq!(
            parse_line(&format!("{P}Successfully unallocated passive skill id: criticals17, name: Critical Chance")),
            Some(Event::Passive { id: "criticals17".into(), allocated: false })
        );
        assert_eq!(parse_line(&format!("{P}#DarthPillars: shes great")), None);
        assert_eq!(
            parse_line(&format!("{P}: Juliana_novachrono has received +30 to [Spirit|Spirit].")),
            Some(Event::Received { character: Some("Juliana_novachrono".into()), text: "+30 to [Spirit|Spirit].".into() })
        );
        assert_eq!(
            parse_line(&format!("{P}: You have received 2 Passive Skill Points.")),
            Some(Event::Received { character: None, text: "2 Passive Skill Points.".into() })
        );
        assert_eq!(parse_line(&format!("{P}: Bob_duo has joined the area.")), Some(Event::Party { character: "Bob_duo".into() }));
        assert_eq!(parse_line(&format!("{P}: Bob_duo a quitté la zone.")), Some(Event::Party { character: "Bob_duo".into() }));
        assert_eq!(parse_line(&format!("{P}Atlas input suspension DISABLED")), Some(Event::Device));
        assert_eq!(parse_line("2026/10/24 02:09:16 ***** LOG FILE OPENING *****"), Some(Event::Session));
    }

    #[test]
    fn clock() {
        // Heure d'été française : le log a deux heures d'avance sur l'UTC.
        assert_eq!(clock_offset(10_000 + 7200, 10_000 - 3), 7200);
        assert_eq!(clock_offset(10_000, 10_000 + 40), 0);
    }

    #[test]
    fn acts() {
        assert_eq!(act_of("P2_Town"), Some(5));
        assert_eq!(act_of("MapCanyon"), Some(6));
        assert_eq!(act_of("MapHideoutShoreline_Claimable"), None);
        assert_eq!(act_of("Sanctum_3_Foyer_3"), None);
    }
}
