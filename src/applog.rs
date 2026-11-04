//! Journal de l'app : `poe2-tracker.log` dans le dossier des données, l'ancien gardé en
//! `poe2-tracker.old.log` au-delà de 2 Mo. Les messages vont aussi sur la sortie d'erreur.
//! Les avertissements et les erreurs sont gardés à part pour le rapport d'erreurs (réglage
//! « Envoyer les erreurs »), un plantage aussi : écrit au prochain démarrage s'il n'a pas pu
//! partir.
//!
//! `log_info!`, `log_warn!`, `log_error!` : comme `format!`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERREUR",
        }
    }
}

/// Avertissement ou erreur à rapporter.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Secondes Unix.
    pub at: i64,
    pub level: Level,
    pub message: String,
}

const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Entrées gardées en attente d'envoi (les plus anciennes partent d'abord).
const MAX_PENDING: usize = 200;

struct Log {
    file: Option<File>,
    path: Option<PathBuf>,
    pending: Vec<Entry>,
}

static LOG: Mutex<Log> = Mutex::new(Log { file: None, path: None, pending: Vec::new() });

pub fn path(dir: &Path) -> PathBuf {
    dir.join("poe2-tracker.log")
}

/// Ouvre le journal dans `dir` et y fait écrire les plantages.
pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = path(dir);
    let crash = previous_crash(&path);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join("poe2-tracker.old.log"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    {
        let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
        log.file = file;
        log.path = Some(path);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        let what = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        write(Level::Error, format!("plantage dans le fil {thread}{at} : {what}\n{}", std::backtrace::Backtrace::force_capture()));
        previous(info);
    }));
    write(Level::Info, format!("{START} {} ({} {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
    // Plantage de la session précédente : il n'a pas pu partir, il partira avec ce démarrage.
    if let Some(text) = crash {
        let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
        log.pending.push(Entry { at: now(), level: Level::Error, message: format!("plantage à la session précédente :\n{text}") });
    }
}

/// Début de session dans le journal.
const START: &str = "démarrage de poe2-tracker";

/// Plantage écrit pendant la dernière session du journal, s'il y en a un.
fn previous_crash(path: &Path) -> Option<String> {
    let text = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&text[text.len().saturating_sub(256 * 1024)..]).into_owned();
    let session = &text[text.rfind(START).unwrap_or(0)..];
    let at = session.find(" plantage dans le fil ")?;
    let from = session[..at].rfind('\n').map_or(0, |p| p + 1);
    Some(session[from..].chars().take(8 * 1024).collect())
}

pub fn write(level: Level, message: String) {
    let at = now();
    let line = format!("{} {:<6} {message}\n", stamp(at), level.tag());
    eprint!("{line}");
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(f) = log.file.as_mut() {
        let _ = f.write_all(line.as_bytes());
    }
    if level >= Level::Warn {
        if log.pending.len() >= MAX_PENDING {
            log.pending.remove(0);
        }
        log.pending.push(Entry { at, level, message });
    }
}

/// Avertissements et erreurs pas encore envoyés (vidés).
pub fn take_pending() -> Vec<Entry> {
    std::mem::take(&mut LOG.lock().unwrap_or_else(|e| e.into_inner()).pending)
}

/// Remet en tête des entrées dont l'envoi a échoué.
pub fn restore_pending(mut entries: Vec<Entry>) {
    let mut log = LOG.lock().unwrap_or_else(|e| e.into_inner());
    entries.append(&mut log.pending);
    let extra = entries.len().saturating_sub(MAX_PENDING);
    entries.drain(..extra);
    log.pending = entries;
}

/// Fin du journal (au plus `max` octets, depuis le début d'une ligne).
pub fn tail(max: usize) -> String {
    let Some(path) = LOG.lock().unwrap_or_else(|e| e.into_inner()).path.clone() else { return String::new() };
    let text = std::fs::read(&path).unwrap_or_default();
    let from = text.len().saturating_sub(max);
    let from = text[from..].iter().position(|&b| b == b'\n').map_or(from, |p| if from == 0 { 0 } else { from + p + 1 });
    String::from_utf8_lossy(&text[from..]).into_owned()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// « 2026-11-04 13:50:12Z » (UTC).
pub fn stamp(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Jours depuis 1970 → date civile (algorithme de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

#[macro_export]
macro_rules! log_info {
    ($($a:tt)*) => { $crate::applog::write($crate::applog::Level::Info, format!($($a)*)) };
}

#[macro_export]
macro_rules! log_warn {
    ($($a:tt)*) => { $crate::applog::write($crate::applog::Level::Warn, format!($($a)*)) };
}

#[macro_export]
macro_rules! log_error {
    ($($a:tt)*) => { $crate::applog::write($crate::applog::Level::Error, format!($($a)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamps() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00Z");
        assert_eq!(stamp(1_793_800_212), "2026-11-04 13:50:12Z");
        assert_eq!(stamp(951_782_400), "2000-02-29 00:00:00Z");
    }
}
