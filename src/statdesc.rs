//! Textes des effets à partir des fichiers de descriptions du jeu (.csd) : pour un ensemble de
//! stats et de valeurs, la phrase affichée en jeu (« Les Monstres Sacrifiés… octroient 5 %… »).
//!
//! Un bloc `description` nomme ses stats, puis donne des variantes : une plage par stat
//! (« 1|# », « #|-1 », « # »), le texte entre guillemets avec {0}, {1:+d}…, et des
//! transformations (« negate 1 »). Chaque langue suit dans un `lang "French"`.

use std::collections::HashMap;

#[derive(Debug)]
struct Variant {
    ranges: Vec<(i64, i64)>,
    text: String,
    /// Transformation et numéro de la stat (à partir de 1).
    handlers: Vec<(String, usize)>,
}

#[derive(Debug)]
struct Block {
    stats: Vec<String>,
    /// Variantes par langue (« English » pour celles du début).
    langs: HashMap<String, Vec<Variant>>,
}

pub struct Descriptions {
    blocks: Vec<Block>,
    /// Blocs où figure chaque stat.
    by_stat: HashMap<String, Vec<usize>>,
}

fn range(s: &str) -> (i64, i64) {
    let bound = |b: &str, default: i64| if b == "#" { default } else { b.parse().unwrap_or(default) };
    match s.split_once('|') {
        Some((lo, hi)) => (bound(lo, i64::MIN), bound(hi, i64::MAX)),
        None if s == "#" => (i64::MIN, i64::MAX),
        // « !0 » : tout sauf zéro, approché par « au moins 1 » ou « au plus -1 » à l'usage.
        None if s.starts_with('!') => (i64::MIN, i64::MAX),
        None => {
            let v = s.parse().unwrap_or(0);
            (v, v)
        }
    }
}

/// Une ligne de variante : plages, texte entre guillemets, transformations.
fn variant(line: &str, n: usize) -> Option<Variant> {
    let (head, rest) = line.split_once('"')?;
    let (text, tail) = rest.rsplit_once('"')?;
    let ranges: Vec<(i64, i64)> = head.split_whitespace().take(n).map(range).collect();
    let mut handlers = Vec::new();
    let mut words = tail.split_whitespace().peekable();
    while let Some(w) = words.next() {
        match words.peek().and_then(|a| a.parse::<usize>().ok()) {
            Some(i) => {
                words.next();
                handlers.push((w.to_string(), i));
            }
            // Mot sans numéro (« canonical_line ») ou suivi d'un texte (« reminderstring X »).
            None if w == "reminderstring" => {
                words.next();
            }
            None => {}
        }
    }
    Some(Variant { ranges, text: text.to_string(), handlers })
}

impl Descriptions {
    /// Texte d'un fichier .csd (déjà décodé de l'UTF-16).
    pub fn parse(text: &str) -> Descriptions {
        let mut blocks: Vec<Block> = Vec::new();
        let mut lines = text.lines().map(str::trim).peekable();
        while let Some(line) = lines.next() {
            if line != "description" && !line.starts_with("description ") {
                continue;
            }
            let Some(head) = lines.next() else { break };
            let mut words = head.split_whitespace();
            let n: usize = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
            let stats: Vec<String> = words.take(n).map(String::from).collect();
            let mut langs = HashMap::new();
            let mut lang = "English".to_string();
            loop {
                let Some(count) = lines.next_if(|l| l.parse::<usize>().is_ok()) else { break };
                let count: usize = count.parse().unwrap();
                let variants: Vec<Variant> = (0..count).filter_map(|_| lines.next()).filter_map(|l| variant(l, n)).collect();
                langs.insert(lang.clone(), variants);
                match lines.next_if(|l| l.starts_with("lang ")) {
                    Some(l) => lang = l.trim_start_matches("lang ").trim_matches('"').to_string(),
                    None => break,
                }
            }
            blocks.push(Block { stats, langs });
        }
        let mut by_stat: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, b) in blocks.iter().enumerate() {
            for s in &b.stats {
                by_stat.entry(s.clone()).or_default().push(i);
            }
        }
        Descriptions { blocks, by_stat }
    }

    /// Phrases pour des stats et leurs valeurs, dans la langue voulue (anglais à défaut). Une stat
    /// sans description donne son identifiant brut.
    pub fn describe(&self, stats: &[(String, i64)], lang: &str) -> Vec<String> {
        let value = |id: &str| stats.iter().find(|(s, _)| s == id).map(|(_, v)| *v);
        let mut done: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for (id, _) in stats {
            if done.contains(&id.as_str()) {
                continue;
            }
            // Le bloc le plus complet dont toutes les stats sont présentes, sinon un bloc de la stat.
            let candidates = self.by_stat.get(id).map(Vec::as_slice).unwrap_or(&[]);
            let block = candidates
                .iter()
                .map(|i| &self.blocks[*i])
                .filter(|b| b.stats.iter().all(|s| value(s).is_some()))
                .max_by_key(|b| b.stats.len())
                .or_else(|| candidates.first().map(|i| &self.blocks[*i]));
            let Some(block) = block else {
                out.push(id.clone());
                continue;
            };
            let values: Vec<i64> = block.stats.iter().map(|s| value(s).unwrap_or(0)).collect();
            done.extend(block.stats.iter().map(String::as_str));
            let variants = block.langs.get(lang).or_else(|| block.langs.get("English"));
            let chosen = variants.and_then(|vs| vs.iter().find(|v| v.ranges.iter().zip(&values).all(|((lo, hi), x)| lo <= x && x <= hi)));
            match chosen {
                Some(v) => out.push(render(v, &values)),
                None if values.iter().all(|v| *v == 0) => {}
                None => out.push(id.clone()),
            }
        }
        out
    }
}

/// Valeur affichée après transformation (« negate », « divide_by_one_hundred »…).
fn shown(v: &Variant, values: &[i64], i: usize) -> f64 {
    let mut x = values.get(i).copied().unwrap_or(0) as f64;
    for (h, k) in &v.handlers {
        if *k != i + 1 {
            continue;
        }
        x = match h.as_str() {
            "negate" => -x,
            "negate_and_double" => -2.0 * x,
            "double" => 2.0 * x,
            "divide_by_two_0dp" => (x / 2.0).trunc(),
            "divide_by_ten_0dp" => (x / 10.0).trunc(),
            "divide_by_ten_1dp" | "divide_by_ten_1dp_if_required" => x / 10.0,
            "divide_by_one_hundred" | "divide_by_one_hundred_2dp" | "divide_by_one_hundred_2dp_if_required" => x / 100.0,
            "divide_by_one_thousand" => x / 1000.0,
            "milliseconds_to_seconds" | "milliseconds_to_seconds_2dp" | "milliseconds_to_seconds_2dp_if_required" => x / 1000.0,
            "deciseconds_to_seconds" => x / 10.0,
            "per_minute_to_per_second" | "per_minute_to_per_second_2dp" => x / 60.0,
            "times_twenty" => x * 20.0,
            _ => x,
        };
    }
    x
}

fn number(x: f64, signed: bool) -> String {
    let body = if x.fract() == 0.0 { format!("{}", x.abs() as i64) } else { format!("{:.2}", x.abs()).trim_end_matches('0').trim_end_matches('.').to_string() };
    match (x < 0.0, signed) {
        (true, _) => format!("-{body}"),
        (false, true) => format!("+{body}"),
        (false, false) => body,
    }
}

/// Texte de la variante, valeurs placées et balises de mise en forme retirées.
fn render(v: &Variant, values: &[i64]) -> String {
    let mut out = String::new();
    let mut rest = v.text.as_str();
    let mut next = 0;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else { break };
        let inner = &rest[open + 1..open + close];
        let (index, fmt) = inner.split_once(':').unwrap_or((inner, ""));
        let i = index.parse().unwrap_or(next);
        next = i + 1;
        out.push_str(&number(shown(v, values, i), fmt.contains('+')));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    strip_markup(&out.replace("\\n", " "))
}

/// « [ContainsRitual|Autels de Rituel] » → « Autels de Rituel », « [Ritual] » → « Ritual ».
pub fn strip_markup(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find(']') else { break };
        let inner = &rest[open + 1..open + close];
        out.push_str(inner.rsplit('|').next().unwrap_or(inner));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSD: &str = "include \"Data/StatDescriptions/map_stat_descriptions.csd\"

description
\t1 map_ritual_tribute_+%
\t2
\t\t1|# \"Monsters Sacrificed at [ContainsRitual|Ritual Altars] grant {0}% increased Tribute\"
\t\t#|-1 \"Monsters Sacrificed at [ContainsRitual|Ritual Altars] grant {0}% reduced Tribute\" negate 1
\tlang \"French\"
\t2
\t\t1|# \"Les Monstres Sacrifiés aux [ContainsRitual|Autels de Rituel] octroient {0}% d'Augmentation de Tribut\"
\t\t#|-1 \"Les Monstres Sacrifiés aux [ContainsRitual|Autels de Rituel] octroient {0}% de Réduction de Tribut\" negate 1

description
\t2 a_min a_max
\t1
\t\t# # \"Adds {0} to {1} damage\"

description
\t1 duration_ms
\t1
\t\t# \"Lasts {0:+d} seconds\" milliseconds_to_seconds 1 reminderstring ReminderTextX
";

    #[test]
    fn describes_stats() {
        let d = Descriptions::parse(CSD);
        let s = |id: &str, v| (id.to_string(), v);
        assert_eq!(d.describe(&[s("map_ritual_tribute_+%", 5)], "French"), vec!["Les Monstres Sacrifiés aux Autels de Rituel octroient 5% d'Augmentation de Tribut"]);
        assert_eq!(d.describe(&[s("map_ritual_tribute_+%", -10)], "English"), vec!["Monsters Sacrificed at Ritual Altars grant 10% reduced Tribute"]);
        // Langue absente : anglais.
        assert_eq!(d.describe(&[s("a_min", 3), s("a_max", 9)], "French"), vec!["Adds 3 to 9 damage"]);
        assert_eq!(d.describe(&[s("duration_ms", 2500)], "English"), vec!["Lasts +2.5 seconds"]);
        assert_eq!(d.describe(&[s("unknown_stat", 1)], "English"), vec!["unknown_stat"]);
    }
}
