//! Textes du jeu en français : noms (objets, uniques, gemmes, zones, classes) et lignes de mods,
//! lus dans les fichiers du jeu et appliqués seulement à l'affichage. Le jeu, lui, reste lu en
//! anglais (Ctrl+C, trade, poe.ninja) : tout ce qui sert de clé garde son texte anglais.
//!
//! Une ligne de mod se traduit par son gabarit : les nombres de la ligne (« +60 », « 10(8-12) »,
//! « (10-20) », « # ») deviennent « # », ce squelette retrouve le gabarit anglais des
//! descriptions de stats, et ses valeurs vont aux mêmes places dans le gabarit français.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::bundles;
use crate::dat::Dat;
use crate::statdesc;

const CACHE: &str = "gametext_fr_v5.json";

/// Descriptions de stats lues : objets, maps, passifs, effets des charmes.
const DESCRIPTIONS: &[&str] = &[
    "data/statdescriptions/stat_descriptions.csd",
    "data/statdescriptions/map_stat_descriptions.csd",
    "data/statdescriptions/endgame_map_stat_descriptions.csd",
    "data/statdescriptions/passive_skill_stat_descriptions.csd",
    "data/statdescriptions/utility_flask_buff_stat_descriptions.csd",
    "data/statdescriptions/tablet_stat_descriptions.csd",
    "data/statdescriptions/gem_stat_descriptions.csd",
];

/// Tables de noms : fichier, colonne du nom (anglais, puis français), un nom anglais qui doit y
/// être (sinon la colonne a changé et la table est laissée de côté).
const NAMES: &[(&str, usize, usize, &str)] = &[
    ("baseitemtypes", 32, 32, "Chaos Orb"),
    // Uniques : le nom français est dans une autre colonne du fichier français.
    ("words", 4, 48, "Goldrim"),
    ("worldareas", 8, 8, "Clearfell"),
    ("itemclasses", 8, 8, "Life Flasks"),
    ("characters", 8, 8, "Sorceress"),
    ("ascendancy", 44, 44, "Chronomancer"),
    ("passiveskills", 50, 50, "Heavy Buffer"),
    // Compétences : nom et description (infobulle des gemmes).
    ("activeskills", 8, 8, "Ground Slam"),
    ("activeskills", 16, 16, "Slams the ground in front of you, creating a wave that travels forward and damages enemies."),
    // Texte d'ambiance des uniques, ligne à ligne (voir `build`).
    ("flavourtext", 10, 10, "Mist of breath\r\nIcing to lips and throat\r\nAs the warm ones choke and fall\r\nUpon the frozen wasteland."),
];

#[derive(Serialize, Deserialize, Default)]
struct Data {
    /// Emplacement des fichiers sources : le cache vaut tant qu'il ne change pas.
    key: String,
    names: HashMap<String, String>,
    /// Gabarits anglais et français d'une même variante, balises retirées, et les plages de
    /// valeurs de ses stats.
    lines: Vec<(String, String, Vec<(i64, i64)>)>,
}

/// Morceau d'un gabarit anglais : une valeur (numéro de la stat) ou un nombre fixe.
enum Part {
    Value(usize),
    Fixed,
}

struct Template {
    parts: Vec<Part>,
    ranges: Vec<(i64, i64)>,
    fr: String,
}

struct GameText {
    names: HashMap<String, String>,
    by_skeleton: HashMap<String, Vec<Template>>,
}

static TEXT: RwLock<Option<Arc<GameText>>> = RwLock::new(None);
/// Lignes déjà traduites (ou pas : `None`), pour ne pas refaire le travail à chaque image.
static MEMO: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(Default::default);

/// Nombre d'une ligne : signe, valeur et plage (« +12 », « 57(49-58) », « (10-20) »), ou « # ».
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    let n = r"\d+(?:\.\d+)?";
    let range = format!(r"\(-?{n}--?{n}\)");
    Regex::new(&format!(r"[+\-−]?(?:{n}(?:{range})?|{range}|#)")).unwrap()
});
/// Valeur d'un gabarit (« {0} », « {1:+d} », « {} »), avec le signe écrit devant.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[+\-]?\{(\d*)(?::([^}]*))?\}|[+\-]?\d+(?:\.\d+)?").unwrap());
/// Suffixe que le jeu ajoute à une ligne copiée.
static SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" \((implicit|enchant|rune|crafted|fractured|desecrated|augmented|unscalable)\)$").unwrap());
static SKILL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Level (\d+) (.+?)(?: \(Max Level (\d+)\))?$").unwrap());

/// Lit les textes français en arrière-plan (cache, sinon les fichiers du jeu), puis redessine.
pub fn start(ctx: eframe::egui::Context, cache: PathBuf, game_dir: PathBuf) {
    std::thread::spawn(move || match load(&cache, &game_dir) {
        Ok(data) => {
            let text = compile(data);
            *TEXT.write().unwrap() = Some(Arc::new(text));
            MEMO.lock().unwrap().clear();
            ctx.request_repaint();
        }
        Err(e) => eprintln!("textes du jeu en français indisponibles : {e}"),
    });
}

fn get() -> Option<Arc<GameText>> {
    if crate::i18n::english() {
        return None;
    }
    TEXT.read().unwrap().clone()
}

/// Nom du jeu (objet, unique, gemme, zone, classe) dans la langue de l'interface.
pub fn name(en: &str) -> Cow<'_, str> {
    match get().and_then(|t| t.names.get(en).cloned()) {
        Some(fr) => Cow::Owned(fr),
        None => Cow::Borrowed(en),
    }
}

/// Ligne de mod ou de stat du jeu dans la langue de l'interface (telle quelle si elle n'est pas
/// reconnue).
pub fn line(en: &str) -> Cow<'_, str> {
    let Some(text) = get() else { return Cow::Borrowed(en) };
    if let Some(done) = MEMO.lock().unwrap().get(en) {
        return done.clone().map_or(Cow::Borrowed(en), Cow::Owned);
    }
    let fr = text.translate(en).or_else(|| text.names.get(en).cloned());
    MEMO.lock().unwrap().insert(en.to_string(), fr.clone());
    fr.map_or(Cow::Borrowed(en), Cow::Owned)
}

/// Lignes d'un texte d'objet dans la langue de l'interface. Le jeu coupe les longues lignes en
/// deux ou trois : une ligne non reconnue est essayée avec les suivantes. Rend, pour chaque
/// ligne affichée, le numéro de la première ligne d'origine et son texte.
pub fn lines(en: &[String]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < en.len() {
        if get().is_some() && line(&en[i]) == en[i].as_str() {
            let joined = (2..=3).filter(|k| i + k <= en.len()).find_map(|k| {
                let text = en[i..i + k].join(" ");
                let fr = line(&text);
                (fr != text.as_str()).then(|| (k, fr.into_owned()))
            });
            if let Some((k, fr)) = joined {
                out.push((i, fr));
                i += k;
                continue;
            }
        }
        out.push((i, line(&en[i]).into_owned()));
        i += 1;
    }
    out
}

/// Compétence donnée par un objet : « Level 15 Freezing Shards (Max Level 18) ».
pub fn skill(en: &str) -> Cow<'_, str> {
    if get().is_none() {
        return Cow::Borrowed(en);
    }
    match SKILL.captures(en) {
        Some(c) => {
            let max = c.get(3).map(|m| format!(" (niveau max {})", m.as_str())).unwrap_or_default();
            Cow::Owned(format!("Niveau {} {}{max}", &c[1], name(&c[2])))
        }
        None => name(en),
    }
}

/// Implicite d'un en-tête : une ligne de mod, ou la compétence donnée par l'objet
/// (« Compétence : Level 15 Freezing Shards »).
pub fn implicit(l: &str) -> Cow<'_, str> {
    match l.strip_prefix(tr!("Octroie l'Aptitude : ")) {
        Some(skill_text) => Cow::Owned(format!("{}{}", tr!("Octroie l'Aptitude : "), skill(skill_text))),
        None => line(l),
    }
}

impl GameText {
    fn translate(&self, en: &str) -> Option<String> {
        let (body, suffix) = match SUFFIX.find(en) {
            Some(m) => (&en[..m.start()], &en[m.start()..]),
            None => (en, ""),
        };
        let values: Vec<&str> = NUMBER.find_iter(body).map(|m| m.as_str()).collect();
        let candidates = self.by_skeleton.get(&skeleton(body))?;
        let fits = |t: &&Template| t.parts.len() == values.len();
        // Plusieurs variantes de même forme (« 1 seconde », « # secondes ») : celle dont les plages
        // acceptent les valeurs (au signe près, le texte montre parfois l'opposé).
        let in_range = |t: &&Template| {
            t.parts.iter().zip(&values).all(|(p, v)| match p {
                Part::Value(i) => match (first_number(v), t.ranges.get(*i)) {
                    (Some(x), Some((lo, hi))) => (*lo as f64..=*hi as f64).contains(&x) || (*lo as f64..=*hi as f64).contains(&-x),
                    _ => true,
                },
                Part::Fixed => true,
            })
        };
        let t = candidates.iter().filter(fits).find(in_range).or_else(|| candidates.iter().find(fits))?;
        let mut by_stat: HashMap<usize, &str> = HashMap::new();
        for (p, v) in t.parts.iter().zip(&values) {
            if let Part::Value(i) = p {
                by_stat.entry(*i).or_insert(v);
            }
        }
        Some(render_fr(&t.fr, &by_stat) + &self.suffix(suffix))
    }

    fn suffix(&self, s: &str) -> String {
        match s.trim() {
            "" => String::new(),
            "(implicit)" => " (implicite)".into(),
            "(augmented)" => " (augmenté)".into(),
            "(rune)" => " (rune)".into(),
            "(enchant)" => " (enchantement)".into(),
            "(crafted)" => " (artisanal)".into(),
            "(fractured)" => " (fracturé)".into(),
            "(desecrated)" => " (profané)".into(),
            other => format!(" {other}"),
        }
    }
}

fn first_number(v: &str) -> Option<f64> {
    let v = v.replace('−', "-");
    let end = v.char_indices().find(|(i, c)| *i > 0 && !(c.is_ascii_digit() || *c == '.')).map_or(v.len(), |(i, _)| i);
    v[..end].trim_start_matches('+').parse().ok()
}

/// La forme d'une ligne : nombres remplacés par « # », espaces simplifiés.
fn skeleton(s: &str) -> String {
    let out = NUMBER.replace_all(s, "#");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Gabarit anglais : sa forme (comme `skeleton`) et la suite de ses nombres.
fn template_parts(text: &str) -> (String, Vec<Part>) {
    let mut parts = Vec::new();
    let mut next = 0;
    let shape = PLACEHOLDER.replace_all(text, |c: &regex::Captures| {
        match c.get(1) {
            Some(i) => {
                let i = i.as_str().parse().unwrap_or(next);
                next = i + 1;
                parts.push(Part::Value(i));
            }
            None => parts.push(Part::Fixed),
        }
        "#"
    });
    (shape.split_whitespace().collect::<Vec<_>>().join(" "), parts)
}

/// Gabarit français rempli avec les valeurs de la ligne anglaise, signe compris.
fn render_fr(fr: &str, values: &HashMap<usize, &str>) -> String {
    let mut out = String::new();
    let mut rest = fr;
    let mut next = 0;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else { break };
        let inner = &rest[open + 1..open + close];
        let (index, fmt) = inner.split_once(':').unwrap_or((inner, ""));
        let i = index.parse().unwrap_or(next);
        next = i + 1;
        let v = values.get(&i).copied().unwrap_or("#");
        let (sign, body) = match v.chars().next() {
            Some(c @ ('+' | '-' | '−')) => (Some(c), &v[c.len_utf8()..]),
            _ => (None, v),
        };
        let negative = matches!(sign, Some('-' | '−'));
        if out.ends_with('+') {
            if negative {
                out.pop();
                out.push(sign.unwrap());
            }
        } else if negative {
            out.push(sign.unwrap());
        } else if fmt.contains('+') {
            out.push('+');
        }
        out.push_str(body);
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    statdesc::strip_markup(&out)
}

fn compile(data: Data) -> GameText {
    let mut by_skeleton: HashMap<String, Vec<Template>> = HashMap::new();
    for (en, fr, ranges) in data.lines {
        let (shape, parts) = template_parts(&en);
        by_skeleton.entry(shape).or_default().push(Template { parts, ranges, fr });
    }
    GameText { names: data.names, by_skeleton }
}

fn key_of(index: &bundles::Index) -> String {
    let tables = NAMES.iter().flat_map(|(t, ..)| [format!("data/balance/{t}.datc64"), format!("data/balance/french/{t}.datc64")]);
    DESCRIPTIONS
        .iter()
        .map(|s| s.to_string())
        .chain(tables)
        .map(|f| index.entry(&f).map(|e| format!("{}:{}:{}", e.bundle, e.offset, e.size)).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",")
}

fn load(cache_dir: &Path, game_dir: &Path) -> Result<Data, String> {
    let index = bundles::Index::open(game_dir).map_err(|e| e.to_string())?;
    let key = key_of(&index);
    let file = cache_dir.join(CACHE);
    if let Some(data) = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<Data>(&s).ok()).filter(|d| d.key == key) {
        return Ok(data);
    }
    let data = build(&index, key);
    let _ = std::fs::write(&file, serde_json::to_string(&data).unwrap_or_default());
    Ok(data)
}

fn build(index: &bundles::Index, key: String) -> Data {
    let mut data = Data { key, ..Default::default() };
    for (table, col_en, col_fr, sample) in NAMES {
        let read = |p: String| index.read(&p).ok().and_then(Dat::new);
        let (Some(en), Some(fr)) = (read(format!("data/balance/{table}.datc64")), read(format!("data/balance/french/{table}.datc64"))) else { continue };
        if en.rows != fr.rows || !(0..en.rows).any(|r| en.string(r, *col_en) == *sample) {
            eprintln!("{table} : colonne des noms introuvable");
            continue;
        }
        for r in 0..en.rows {
            let (e, f) = (en.string(r, *col_en), fr.string(r, *col_fr));
            // Texte à trous (« Spectre: {0} ») ou accordé selon le genre (morceaux de noms de rares) :
            // pas un nom à remplacer tel quel.
            if e.is_empty() || f.is_empty() || e == f || f.contains("<if:") || f.contains('{') {
                continue;
            }
            // Un texte sur plusieurs lignes arrive ligne à ligne dans le texte d'un objet.
            let (el, fl): (Vec<&str>, Vec<&str>) = (e.lines().collect(), f.lines().collect());
            if el.len() > 1 && el.len() == fl.len() {
                for (a, b) in el.into_iter().zip(fl) {
                    data.names.entry(a.trim().to_string()).or_insert_with(|| b.trim().to_string());
                }
            } else {
                data.names.entry(e).or_insert(f);
            }
        }
    }
    for file in DESCRIPTIONS {
        let Ok(raw) = index.read(file) else { continue };
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let text = String::from_utf16_lossy(&units);
        data.lines.extend(pairs(&text));
    }
    data
}

/// Couples (gabarit anglais, gabarit français, plages) d'un fichier de descriptions. Une variante
/// sur plusieurs lignes donne un couple par ligne.
fn pairs(text: &str) -> Vec<(String, String, Vec<(i64, i64)>)> {
    let mut out = Vec::new();
    let mut lines = text.lines().map(str::trim).peekable();
    while let Some(line) = lines.next() {
        if line != "description" && !line.starts_with("description ") {
            continue;
        }
        let Some(head) = lines.next() else { break };
        let n: usize = head.split_whitespace().next().and_then(|w| w.parse().ok()).unwrap_or(0);
        let mut english: Vec<statdesc::Variant> = Vec::new();
        let mut french: Vec<statdesc::Variant> = Vec::new();
        let mut lang = "English".to_string();
        loop {
            let Some(count) = lines.next_if(|l| l.parse::<usize>().is_ok()) else { break };
            let count: usize = count.parse().unwrap();
            let variants: Vec<statdesc::Variant> = (0..count).filter_map(|_| lines.next()).filter_map(|l| statdesc::variant(l, n)).collect();
            match lang.as_str() {
                "English" => english = variants,
                "French" => french = variants,
                _ => {}
            }
            match lines.next_if(|l| l.starts_with("lang ")) {
                Some(l) => lang = l.trim_start_matches("lang ").trim_matches('"').to_string(),
                None => break,
            }
        }
        for (i, e) in english.iter().enumerate() {
            // La variante française de mêmes plages, à défaut celle du même rang.
            let Some(f) = french.iter().find(|f| f.ranges == e.ranges).or_else(|| french.get(i)) else { continue };
            let (es, fs): (Vec<&str>, Vec<&str>) = (e.text.split("\\n").collect(), f.text.split("\\n").collect());
            if es.len() != fs.len() {
                continue;
            }
            for (a, b) in es.into_iter().zip(fs) {
                out.push((statdesc::strip_markup(a), b.to_string(), e.ranges.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GameText {
        let csd = "description
\t2 min max
\t1
\t\t# # \"Adds {0} to {1} [Fire] Damage\"
\tlang \"French\"
\t1
\t\t# # \"Ajoute {0} à {1} Dégâts de [Fire|feu]\"

description
\t1 life
\t1
\t\t# \"{0:+d} to maximum Life\"
\tlang \"French\"
\t1
\t\t# \"{0:+d} à la Vie maximale\"

description
\t1 dur
\t2
\t\t1 \"Lasts {0} second\"
\t\t#|-1 \"Lasts {0} seconds\"
\tlang \"French\"
\t2
\t\t1 \"Dure {0} seconde\"
\t\t#|-1 \"Dure {0} secondes\"

description
\t1 speed
\t2
\t\t1|# \"{0}% increased Movement Speed\"
\t\t#|-1 \"{0}% reduced Movement Speed\" negate 1
\tlang \"French\"
\t2
\t\t1|# \"{0}% d'augmentation de la Vitesse de déplacement\"
\t\t#|-1 \"{0}% de réduction de la Vitesse de déplacement\" negate 1
";
        compile(Data { lines: pairs(csd), ..Default::default() })
    }

    #[test]
    fn translates_mod_lines() {
        let t = sample();
        assert_eq!(t.translate("Adds 4 to 87 Fire Damage").as_deref(), Some("Ajoute 4 à 87 Dégâts de feu"));
        assert_eq!(t.translate("+60 to maximum Life").as_deref(), Some("+60 à la Vie maximale"));
        assert_eq!(t.translate("−76 to maximum Life").as_deref(), Some("−76 à la Vie maximale"));
        assert_eq!(t.translate("+(80-89) to maximum Life (implicit)").as_deref(), Some("+(80-89) à la Vie maximale (implicite)"));
        assert_eq!(t.translate("Lasts 1 second").as_deref(), Some("Dure 1 seconde"));
        assert_eq!(t.translate("Lasts 3 seconds").as_deref(), Some("Dure 3 secondes"));
        assert_eq!(t.translate("15% reduced Movement Speed").as_deref(), Some("15% de réduction de la Vitesse de déplacement"));
        assert_eq!(t.translate("#% increased Movement Speed").as_deref(), Some("#% d'augmentation de la Vitesse de déplacement"));
        assert_eq!(t.translate("30(25-30)% increased Movement Speed").as_deref(), Some("30(25-30)% d'augmentation de la Vitesse de déplacement"));
        assert_eq!(t.translate("Something unknown"), None);
    }

    /// Tous les textes du jeu installé : statistiques de reconnaissance sur l'historique local.
    /// `cargo test gametext_from_game -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn gametext_from_game() {
        let game = crate::paths::detect().expect("dossier du jeu");
        let index = bundles::Index::open(&game).unwrap();
        let data = build(&index, String::new());
        eprintln!("{} noms, {} gabarits", data.names.len(), data.lines.len());
        let t = compile(data);
        for l in ["+60 to maximum Life", "Adds 4 to 87 Lightning Damage", "20% increased Mana Regeneration Rate", "+28% to Fire Resistance", "4.1 Life Regeneration per second", "Has 1 Charm Slot", "+2 to Level of all Spell Skills", "30(25-30)% increased Movement Speed"] {
            eprintln!("{l} → {:?}", t.translate(l));
        }
        for n in ["Chaos Orb", "Goldrim", "Gelid Staff", "Sinkhole", "Sorceress", "Chronomancer"] {
            eprintln!("{n} → {:?}", t.names.get(n));
        }
        // Lignes de vrais objets (historique au format de l'app : `POE2_ITEMS=chemin/history.json`).
        if let Ok(path) = std::env::var("POE2_ITEMS") {
            let raws: Vec<String> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let (mut ok, mut total) = (0, 0);
            for it in raws.iter().filter_map(|r| crate::item::parse(r)) {
                let mut k = 0;
                while k < it.mods.len() {
                    total += 1;
                    let found = (1..=3).filter(|n| k + n <= it.mods.len()).find(|n| {
                        let text = it.mods[k..k + n].join(" ");
                        t.translate(&text).or_else(|| t.names.get(&text).cloned()).is_some()
                    });
                    match found {
                        Some(n) => {
                            ok += 1;
                            k += n;
                        }
                        None => {
                            eprintln!("  non traduit : {}", it.mods[k]);
                            k += 1;
                        }
                    }
                }
            }
            eprintln!("{ok}/{total} lignes traduites");
        }
    }
}
