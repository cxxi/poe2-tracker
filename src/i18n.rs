//! Langue de l'interface : le français sert de clé, l'anglais est une table de correspondance.
//!
//! Le jeu reste lu en anglais (Ctrl+C, trade, poe.ninja) : seul ce qui s'affiche change.
//!
//! - `tr!("Réglages")` : le texte dans la langue choisie ;
//! - `tr!("{n} annonces", n)` ou `tr!("{n} annonces", n = total)` : avec des valeurs, nommées
//!   dans le texte (l'anglais peut les placer ailleurs) ;
//! - `trn!(n, "{n} mort", "{n} morts")` : singulier ou pluriel selon la langue ;
//! - `tr!("Armure@torse")` : un mot français à plusieurs sens (le contexte après « @ » ne
//!   s'affiche pas) ;
//! - `tkey!("▲ MIEUX")` : un texte gardé tel quel (il sert aussi de repère dans le code), traduit
//!   plus tard à l'affichage par `tr_dyn`.
//!
//! Chaque texte est vérifié par un test : il doit avoir sa traduction anglaise.

mod en;

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    /// Code enregistré dans les réglages.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// Nom de la langue, dans cette langue.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Langue du système : français s'il est en français, anglais sinon.
    pub fn system() -> Lang {
        if system_locale().is_some_and(|l| l.to_lowercase().starts_with("fr")) { Lang::Fr } else { Lang::En }
    }
}

#[cfg(windows)]
fn system_locale() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER).open_subkey("Control Panel\\International").ok()?.get_value("LocaleName").ok()
}

#[cfg(not(windows))]
fn system_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty() && v != "C" && v != "POSIX")
}

static ENGLISH: AtomicBool = AtomicBool::new(false);
/// L'autre langue le temps qu'Alt est maintenu.
static FLIP: AtomicBool = AtomicBool::new(false);

pub fn set(lang: Lang) {
    ENGLISH.store(lang == Lang::En, Ordering::Relaxed);
}

/// Langue choisie dans les réglages.
pub fn base() -> Lang {
    if ENGLISH.load(Ordering::Relaxed) { Lang::En } else { Lang::Fr }
}

/// Bascule dans l'autre langue (Alt maintenu) ou y revient. Vrai si l'affichage change.
pub fn set_flip(on: bool) -> bool {
    FLIP.swap(on, Ordering::Relaxed) != on
}

/// Langue affichée en anglais : celle des réglages, ou l'autre tant qu'Alt est maintenu.
pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed) != FLIP.load(Ordering::Relaxed)
}

static EN: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| en::EN.iter().copied().collect());

/// Le texte dans la langue choisie (le français tel quel, sinon sa traduction ; à défaut, le
/// français).
pub fn tr(fr: &'static str) -> &'static str {
    tr_dyn(fr)
}

/// `tr` pour un texte connu seulement à l'exécution (repère `tkey!`, libellé rangé dans une
/// structure).
pub fn tr_dyn(fr: &str) -> &str {
    if !english() {
        // « Armure@torse » : le même mot que « Armure » en français, pas en anglais.
        return fr.split_once('@').map_or(fr, |(text, _)| text);
    }
    EN.get(fr).copied().unwrap_or(fr)
}

/// Remplace les `{nom}` du texte par les valeurs données.
pub fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').map(|close| (&after[..close], close)) {
            Some((name, close)) if args.iter().any(|(k, _)| *k == name) => {
                let (_, v) = args.iter().find(|(k, _)| *k == name).unwrap();
                out.push_str(&v.to_string());
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Pluriel selon la langue : en français à partir de 2, en anglais sauf pour 1.
pub fn plural_form(n: f64) -> bool {
    if english() { n != 1.0 } else { n >= 2.0 }
}

/// Pourcentage écrit dans la langue : « 12 % » en français, « 12% » en anglais.
pub fn percent(v: impl Display) -> String {
    if english() { format!("{v}%") } else { format!("{v} %") }
}

/// Nombre décimal écrit dans la langue : virgule en français, point en anglais.
pub fn decimal(s: String) -> String {
    if english() { s } else { s.replace('.', ",") }
}

#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($k:ident $(= $v:expr)?),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($s), &[$((stringify!($k), $crate::tr_arg!($k $(= $v)?))),+])
    };
}

#[macro_export]
macro_rules! trn {
    ($n:expr, $one:literal, $many:literal $(, $k:ident $(= $v:expr)?)* $(,)?) => {{
        let n = $n;
        let text = if $crate::i18n::plural_form(n as f64) { $crate::i18n::tr($many) } else { $crate::i18n::tr($one) };
        $crate::i18n::fill(text, &[("n", &n as &dyn std::fmt::Display) $(, (stringify!($k), $crate::tr_arg!($k $(= $v)?)))*])
    }};
}

#[macro_export]
macro_rules! tr_arg {
    ($k:ident) => {
        &$k as &dyn std::fmt::Display
    };
    ($k:ident = $v:expr) => {
        &$v as &dyn std::fmt::Display
    };
}

#[macro_export]
macro_rules! tkey {
    ($s:literal) => {
        $s
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_named_values() {
        let n = 3;
        assert_eq!(fill("{n} annonces, {x}", &[("n", &n), ("x", &"ok")]), "3 annonces, ok");
        // Accolade sans valeur : laissée telle quelle.
        assert_eq!(fill("{0} a {inconnu}", &[]), "{0} a {inconnu}");
    }

    /// Littéraux de `tr!`, `trn!` et `tkey!` d'un texte source (le texte brut, échappements compris).
    fn keys_in(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (mac, count) in [("tr!(", 1), ("trn!(", 2), ("tkey!(", 1)] {
            let mut rest = src;
            while let Some(i) = rest.find(mac) {
                // `tr!(` ne doit pas être la fin d'un autre nom (`ntr!(`) : on regarde avant.
                let before = rest[..i].chars().last();
                rest = &rest[i + mac.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let mut s = rest;
                for _ in 0..count {
                    let Some(q) = s.find('"') else { break };
                    s = &s[q + 1..];
                    let mut end = 0;
                    let bytes = s.as_bytes();
                    while end < bytes.len() && bytes[end] != b'"' {
                        end += if bytes[end] == b'\\' { 2 } else { 1 };
                    }
                    out.push(s[..end].to_string());
                    s = &s[end + 1..];
                }
            }
        }
        out
    }

    /// Clés de la table anglaise, telles qu'écrites dans `en.rs`.
    fn table_keys() -> Vec<(String, String)> {
        let src = include_str!("i18n/en.rs");
        let pair = regex::Regex::new(r#"(?m)^\s*\("((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\),"#).unwrap();
        pair.captures_iter(src).map(|c| (c[1].to_string(), c[2].to_string())).collect()
    }

    fn placeholders(s: &str) -> Vec<String> {
        let re = regex::Regex::new(r"\{([a-z_][a-z0-9_]*)\}").unwrap();
        let mut v: Vec<String> = re.captures_iter(s).map(|c| c[1].to_string()).collect();
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn every_text_is_translated() {
        let table = table_keys();
        let known: std::collections::HashSet<&str> = table.iter().map(|(k, _)| k.as_str()).collect();
        let mut used = std::collections::HashSet::new();
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            for k in keys_in(&src) {
                if !known.contains(k.as_str()) {
                    missing.push(format!("{} : {k}", path.file_name().unwrap().to_string_lossy()));
                }
                used.insert(k);
            }
        }
        assert!(missing.is_empty(), "textes sans traduction :\n{}", missing.join("\n"));
        let unused: Vec<&str> = known.iter().filter(|k| !used.contains(**k)).copied().collect();
        assert!(unused.is_empty(), "traductions inutilisées :\n{}", unused.join("\n"));
        let mut seen = std::collections::HashSet::new();
        for (fr, en) in &table {
            assert!(seen.insert(fr.as_str()), "clé en double : {fr}");
            assert_eq!(placeholders(fr), placeholders(en), "valeurs différentes : {fr} / {en}");
        }
    }
}
