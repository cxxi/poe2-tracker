//! Comparaison d'un objet copié avec la pièce suivie au même emplacement.

use std::sync::LazyLock;

use regex::Regex;

use std::collections::BTreeMap;

use crate::augments::BaseMeta;
use crate::gear::{self, CHARMS, Gear, Kind, RINGS, Slot};
use crate::guide::Step;
use crate::item::{Item, Rarity};

/// (clé, libellé, motif, poids). Le poids traduit l'importance pour un lanceur de sorts de
/// froid ; il sert à trancher, les écarts bruts restent affichés.
const STATS: &[(&str, &str, &str, f64)] = &[
    ("spell_lvl", "niv. sorts", r"^\+(\d+) to Level of all Spell Skills$", 10.0),
    ("cold_lvl", "niv. sorts de froid", r"^\+(\d+) to Level of all Cold Spell Skills$", 8.0),
    ("spell_dmg", "% dégâts de sort", r"^(\d+)% increased Spell Damage$", 0.15),
    ("cold_dmg", "% dégâts de froid", r"^(\d+)% increased Cold Damage$", 0.12),
    ("extra_dmg", "% dégâts en plus", r"^Gain (\d+)% of Damage as Extra \w+ Damage$", 0.3),
    ("cast_speed", "% vitesse d'incantation", r"^(\d+)% increased Cast Speed$", 0.6),
    ("spell_crit", "% critique des sorts", r"^(\d+)% increased Critical Hit Chance for Spells$", 0.08),
    ("crit_dmg", "% dégâts critiques", r"^(\d+)% increased Critical Damage Bonus$", 0.08),
    ("life", "vie", r"^\+(\d+) to maximum Life$", 0.12),
    ("mana", "mana", r"^\+(\d+) to maximum Mana$", 0.04),
    ("spirit", "esprit", r"^\+(\d+) to Spirit$", 0.3),
    ("fire_res", "% rés. feu", r"^\+(\d+)% to Fire Resistance$", 0.15),
    ("cold_res", "% rés. froid", r"^\+(\d+)% to Cold Resistance$", 0.15),
    ("light_res", "% rés. foudre", r"^\+(\d+)% to Lightning Resistance$", 0.15),
    ("chaos_res", "% rés. chaos", r"^\+(\d+)% to Chaos Resistance$", 0.1),
    ("all_res", "% toutes rés.", r"^\+(\d+)% to all Elemental Resistances$", 0.45),
    ("move", "% vitesse de déplacement", r"^(\d+)% increased Movement Speed$", 1.2),
    ("int", "intelligence", r"^\+(\d+) to Intelligence$", 0.04),
    ("str", "force", r"^\+(\d+) to Strength$", 0.03),
    ("dex", "dextérité", r"^\+(\d+) to Dexterity$", 0.02),
    ("all_attr", "tous attributs", r"^\+(\d+) to all Attributes$", 0.09),
    ("mana_regen", "% régén. mana", r"^(\d+)% increased Mana Regeneration Rate$", 0.05),
];
const DEFENCE: &[(&str, &str, f64)] =
    &[("Energy Shield", "bouclier d'énergie", 0.08), ("Armour", "armure", 0.01), ("Evasion Rating", "évasion", 0.01)];

static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| STATS.iter().map(|s| Regex::new(s.2).unwrap()).collect());

/// Valeurs des stats utiles, dans l'ordre de `STATS` puis `DEFENCE`.
pub fn stats(it: &Item) -> Vec<f64> {
    let mut out: Vec<f64> = PATTERNS
        .iter()
        .map(|re| it.mods.iter().filter_map(|m| re.captures(m)).map(|c| c[1].parse::<f64>().unwrap_or(0.0)).sum())
        .collect();
    out.extend(DEFENCE.iter().map(|(p, _, _)| it.props.get(*p).copied().unwrap_or(0.0)));
    out
}

fn weights() -> impl Iterator<Item = f64> {
    STATS.iter().map(|s| s.3).chain(DEFENCE.iter().map(|d| d.2))
}

fn labels() -> impl Iterator<Item = &'static str> {
    STATS.iter().map(|s| s.1).chain(DEFENCE.iter().map(|d| d.1))
}

pub fn score_stats(st: &[f64]) -> f64 {
    st.iter().zip(weights()).map(|(v, w)| v * w).sum()
}

pub fn score(it: &Item) -> f64 {
    score_stats(&stats(it))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Better,
    Worse,
    Equal,
    /// Pas de référence fiable (rien de suivi, nouveau type de charme…).
    Unknown,
    /// C'est la pièce déjà suivie.
    Same,
}

#[derive(Clone, Debug)]
pub struct Delta {
    pub text: String,
    /// Positif = mieux pour toi, négatif = moins bien, 0 = neutre.
    pub sign: i8,
    /// Constat sur la pièce entière (sa base), affiché à part avant les écarts ligne à ligne.
    pub headline: bool,
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub verdict: Verdict,
    /// Emplacements comparés (à allumer dans la grille).
    pub against: Vec<Slot>,
    pub against_name: String,
    pub deltas: Vec<Delta>,
    pub notes: Vec<String>,
    /// Référence du jugement : « Ky Ice Nova — Act 3 », ou vide (jugement générique).
    pub basis: String,
}

impl Comparison {
    fn unknown(note: String, against: Vec<Slot>) -> Self {
        Comparison { verdict: Verdict::Unknown, against, against_name: String::new(), deltas: vec![], notes: vec![note], basis: String::new() }
    }
}

fn fmt_num(x: f64) -> String {
    format!("{x}").replace('.', ",")
}

fn ratio_verdict(new: f64, old: f64) -> Verdict {
    if old <= 0.0 {
        return Verdict::Unknown;
    }
    let r = new / old;
    if r > 1.1 {
        Verdict::Better
    } else if r < 0.9 {
        Verdict::Worse
    } else {
        Verdict::Equal
    }
}

fn uses(it: &Item) -> f64 {
    match (it.props.get("charges_use"), it.props.get("charges_max")) {
        (Some(u), Some(m)) if *u > 0.0 => (m / u).floor(),
        _ => 0.0,
    }
}

fn delta(text: String, sign: i8) -> Delta {
    Delta { text, sign, headline: false }
}

/// Fioles : comme les dégâts pour une arme, la quantité rendue par gorgée passe avant tout.
/// Durée, utilisations et mods ne départagent que deux fioles qui rendent autant.
fn compare_flask(it: &Item, worn: &Item) -> (Verdict, Vec<Delta>) {
    let get = |i: &Item, k: &str| i.props.get(k).copied().unwrap_or(0.0);
    let (r_new, r_old) = (get(it, "recover"), get(worn, "recover"));
    let mut d = vec![delta(
        format!("rend {} au lieu de {}{}", fmt_num(r_new), fmt_num(r_old), pct(r_new, r_old)),
        (r_new - r_old).signum() as i8,
    )];
    let (t_new, t_old) = (get(it, "recover_time"), get(worn, "recover_time"));
    if t_new != t_old {
        d.push(delta(format!("en {} s au lieu de {} s", fmt_num(t_new), fmt_num(t_old)), (t_old - t_new).signum() as i8));
    }
    let (u_new, u_old) = (uses(it), uses(worn));
    if u_new != u_old {
        d.push(delta(format!("{u_new} utilisations au lieu de {u_old}"), (u_new - u_old).signum() as i8));
    }
    for m in it.mods.iter().filter(|m| !worn.mods.contains(m)) {
        d.push(delta(format!("en plus : {m}"), 0));
    }
    for m in worn.mods.iter().filter(|m| !it.mods.contains(m)) {
        d.push(delta(format!("en moins : {m}"), 0));
    }
    let verdict = match ratio_verdict(r_new, r_old) {
        Verdict::Equal => {
            let rest = |i: &Item| uses(i).min(6.0).sqrt() / get(i, "recover_time").max(0.5).powf(0.25);
            ratio_verdict(rest(it), rest(worn))
        }
        v => v,
    };
    (verdict, d)
}

fn compare_charm(it: &Item, worn: &Item) -> (Verdict, Vec<Delta>) {
    let dur = |i: &Item| i.props.get("duration").copied().unwrap_or(1.0);
    let mut d = Vec::new();
    if dur(it) != dur(worn) {
        d.push(delta(format!("dure {} s au lieu de {} s", fmt_num(dur(it)), fmt_num(dur(worn))), (dur(it) - dur(worn)).signum() as i8));
    }
    if uses(it) != uses(worn) {
        d.push(delta(format!("{} utilisations au lieu de {}", uses(it), uses(worn)), (uses(it) - uses(worn)).signum() as i8));
    }
    for m in it.mods.iter().filter(|m| !worn.mods.contains(m)) {
        d.push(delta(format!("en plus : {m}"), 1));
    }
    let score = |i: &Item| dur(i) * uses(i).max(1.0);
    (ratio_verdict(score(it), score(worn)), d)
}

// --- Dégâts des armes d'attaque ------------------------------------------------------

/// Dégâts par seconde d'une arme d'attaque (physique, élémentaire + chaos), d'après ses
/// lignes de base, mods locaux compris. `None` pour un bâton de sorts, un bouclier…
#[derive(Clone, Copy, Default)]
struct Dps {
    phys: f64,
    ele: f64,
}

impl Dps {
    fn total(&self) -> f64 {
        self.phys + self.ele
    }
}

fn weapon_dps(it: &Item) -> Option<Dps> {
    let aps = *it.props.get("aps")?;
    let get = |k: &str| it.props.get(k).copied().unwrap_or(0.0);
    let dps = Dps { phys: get("phys_avg") * aps, ele: get("ele_avg") * aps };
    (dps.total() > 0.0).then_some(dps)
}

fn pct(new: f64, old: f64) -> String {
    if old <= 0.0 {
        return String::new();
    }
    let p = ((new / old - 1.0) * 100.0).round();
    format!(" ({}{} %)", if p >= 0.0 { "+" } else { "−" }, p.abs())
}

/// Lignes « DPS » en tête du diagnostic et verdict imposé par les dégâts de l'arme : c'est la
/// priorité pour un build d'attaque, comme les dégâts de sort pour un lanceur de sorts.
/// Part des dégâts physiques et élémentaires dans le jugement d'une arme d'attaque, d'après
/// les mods que vise le build : un build élémentaire tire peu de la partie physique.
fn damage_focus(step: Option<&Step>) -> (f64, f64) {
    let Some(step) = step else { return (1.0, 1.0) };
    let attack = |t: &&crate::guide::Target| !t.template.contains("Resistance") && !t.template.contains("Spell");
    let phys = step.all_targets().filter(attack).any(|t| t.template.contains("Physical"));
    let ele = step.all_targets().filter(attack).any(|t| ["Fire", "Cold", "Lightning", "Elemental"].iter().any(|e| t.template.contains(e)));
    match (phys, ele) {
        (true, false) => (1.0, 0.3),
        (false, true) => (0.3, 1.0),
        _ => (1.0, 1.0),
    }
}

fn weapon_rows(it: &Item, refs: &[&Item], focus: (f64, f64)) -> (Option<Verdict>, Vec<Delta>) {
    let Some(new) = weapon_dps(it) else { return (None, vec![]) };
    let olds: Vec<Dps> = refs.iter().filter_map(|i| weapon_dps(i)).collect();
    let r1 = |x: f64| fmt_num((x * 10.0).round() / 10.0);
    if olds.is_empty() {
        return (None, vec![delta(format!("DPS de l'arme : {}", r1(new.total())), 0)]);
    }
    let old = olds.iter().fold(Dps::default(), |a, d| Dps { phys: a.phys + d.phys, ele: a.ele + d.ele });
    let mut rows = vec![delta(
        format!("DPS {} au lieu de {}{}", r1(new.total()), r1(old.total()), pct(new.total(), old.total())),
        (new.total() - old.total()).signum() as i8,
    )];
    if new.phys > 0.0 || old.phys > 0.0 {
        let range = |i: &Item| match (i.props.get("phys_min"), i.props.get("phys_max")) {
            (Some(a), Some(b)) => format!("{a}-{b}"),
            _ => "0".into(),
        };
        let worn_range = refs.iter().find(|i| weapon_dps(i).is_some()).map(|i| range(i)).unwrap_or_default();
        rows.push(delta(
            format!("physique {} au lieu de {} (DPS {} / {})", range(it), worn_range, r1(new.phys), r1(old.phys)),
            (new.phys - old.phys).signum() as i8,
        ));
    }
    if (new.ele - old.ele).abs() > 0.05 {
        rows.push(delta(format!("élémentaire DPS {} au lieu de {}", r1(new.ele), r1(old.ele)), (new.ele - old.ele).signum() as i8));
    }
    let weighted = |d: &Dps| d.phys * focus.0 + d.ele * focus.1;
    if focus != (1.0, 1.0) {
        let what = if focus.0 > focus.1 { "physiques" } else { "élémentaires" };
        rows.push(delta(format!("le build mise sur les dégâts {what} : l'autre partie compte peu"), 0));
    }
    let verdict = match ratio_verdict(weighted(&new), weighted(&old)) {
        Verdict::Equal | Verdict::Unknown => None,
        v => Some(v),
    };
    (verdict, rows)
}

// --- Jugement selon le build -------------------------------------------------------

/// Poids d'un mod visé ailleurs que sur cet emplacement (utile, mais pas la priorité ici).
const ELSEWHERE: f64 = 0.3;
/// Un mod qui dépasse la cible ne compte pas plus de 1,5 fois.
const CAP: f64 = 1.5;
const DEFENCES: &[(&str, &str)] = &[("Energy Shield", "Energy Shield"), ("Armour", "Armour"), ("Evasion Rating", "Evasion")];

/// Modèle de stat -> valeur cumulée, pour tous les mods d'un ou plusieurs objets.
fn mod_values<'a>(items: impl IntoIterator<Item = &'a Item>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for it in items {
        for m in &it.mods {
            let (t, v) = crate::trade::template(m);
            if let Some(v) = v {
                *out.entry(t).or_insert(0.0) += v;
            }
        }
    }
    out
}

fn render(template: &str, d: f64) -> String {
    let sign = if d > 0.0 { "+" } else { "−" };
    let n = fmt_num((d.abs() * 10.0).round() / 10.0);
    template.replacen('#', &format!("{sign}{n}"), 1).replace('#', "…")
}

struct Profile {
    /// Modèle -> valeur visée sur cet emplacement (implicites de la base visée compris).
    here: BTreeMap<String, f64>,
    /// Modèle -> plus grande valeur visée ailleurs dans le build.
    elsewhere: BTreeMap<String, f64>,
    /// Défenses que le build recherche (bouclier d'énergie, armure, évasion).
    defences: Vec<&'static str>,
    /// Arme de caster : les lignes qui font sa puissance (niveaux et dégâts des sorts…)
    /// pèsent plus que le reste.
    caster: bool,
}

/// Lignes qui font la puissance d'une arme de caster, faute de DPS.
static CASTER_CORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Level of all .*Spell Skills|Spell Damage|^#% increased (Fire|Cold|Lightning|Chaos|Elemental) Damage$|Cast Speed|Critical Hit Chance for Spells|Damage as Extra").unwrap()
});
/// Poids de ces lignes sur une arme de caster.
const CASTER_CORE_WEIGHT: f64 = 2.5;

/// Ligne d'implicite RePoE (« +(20-30)% to Cold Resistance ») -> modèle et valeur moyenne.
fn implicit_template(line: &str) -> (String, Option<f64>) {
    static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((\d+(?:\.\d+)?)-(\d+(?:\.\d+)?)\)").unwrap());
    let mid = RANGE.replace_all(line, |c: &regex::Captures| {
        let (a, b) = (c[1].parse::<f64>().unwrap_or(0.0), c[2].parse::<f64>().unwrap_or(0.0));
        format!("{}", ((a + b) / 2.0).round())
    });
    crate::trade::template(&mid)
}

impl Profile {
    /// `implicits` : implicites de la base visée, voulus comme les mods visés.
    fn new(step: &Step, slots: &[Slot], implicits: &[String], caster: bool) -> Option<Profile> {
        let mut here = BTreeMap::new();
        for s in slots {
            for t in step.targets_for(*s) {
                let e = here.entry(t.template.clone()).or_insert(0.0_f64);
                *e = e.max(t.value);
            }
        }
        if here.is_empty() {
            return None;
        }
        for line in implicits {
            if let (t, Some(v)) = implicit_template(line) {
                let e = here.entry(t).or_insert(0.0_f64);
                *e = e.max(v);
            }
        }
        let mut elsewhere = BTreeMap::new();
        for t in step.all_targets() {
            let e = elsewhere.entry(t.template.clone()).or_insert(0.0_f64);
            *e = e.max(t.value);
        }
        let defences = DEFENCES
            .iter()
            .filter(|(_, word)| step.all_targets().any(|t| t.template.contains(word)))
            .map(|(prop, _)| *prop)
            .collect();
        Some(Profile { here, elsewhere, defences, caster })
    }

    /// Poids et valeur de référence d'un modèle, ou `None` s'il est hors build.
    fn weight(&self, template: &str) -> Option<(f64, f64)> {
        let core = if self.caster && CASTER_CORE.is_match(template) { CASTER_CORE_WEIGHT } else { 1.0 };
        if let Some(v) = self.here.get(template) {
            return Some((core, *v));
        }
        self.elsewhere.get(template).map(|v| (ELSEWHERE * core, *v))
    }

    fn score(&self, values: &BTreeMap<String, f64>) -> f64 {
        values
            .iter()
            .filter_map(|(t, v)| self.weight(t).map(|(w, target)| w * (v / target.max(1e-6)).min(CAP)))
            .sum()
    }
}

/// Où en est une pièce sur une ligne visée par le build.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Have {
    /// Au niveau visé (ou au-dessus).
    Full(f64),
    /// Présente, mais en dessous.
    Partial(f64),
    Missing,
    /// Pas de pièce à comparer (case vide).
    Unknown,
}

/// Défenses d'une pièce d'armure : nom de la propriété en en-tête, mod plat local, et mods
/// en % qui l'augmentent (y compris les hybrides).
const ARMOUR_DEFENCES: [(&str, &str, &[&str]); 3] = [
    ("Armour", "# to Armour", &["#% increased Armour", "#% increased Armour and Evasion", "#% increased Armour and Energy Shield", "#% increased Armour, Evasion and Energy Shield"]),
    ("Evasion Rating", "# to Evasion Rating", &["#% increased Evasion Rating", "#% increased Armour and Evasion", "#% increased Evasion and Energy Shield", "#% increased Armour, Evasion and Energy Shield"]),
    ("Energy Shield", "# to maximum Energy Shield", &["#% increased Energy Shield", "#% increased Armour and Energy Shield", "#% increased Evasion and Energy Shield", "#% increased Armour, Evasion and Energy Shield"]),
];

/// Lignes que le build vise sur `slot`, chacune avec ce qu'en a la pièce `it`.
///
/// Sur une pièce d'armure, les mods de défense visés sont locaux : ils s'ajoutent à la défense
/// du type de base. On compare donc la défense totale de la pièce (son en-tête) à celle de la
/// pièce visée, (base visée + plat) × (1 + %), plutôt que ligne par ligne. `base_defences` :
/// défenses (armure, évasion, bouclier d'énergie) d'un type de base.
pub fn check_targets(it: &Item, step: &Step, slot: Slot, base_defences: impl Fn(&str) -> Option<[f64; 3]>) -> Vec<(String, Have)> {
    let values = mod_values([it]);
    let armour_piece = matches!(slot.in_set(1), Slot::Helmet | Slot::Body | Slot::Gloves | Slot::Boots | Slot::Offhand | Slot::Offhand2)
        && ARMOUR_DEFENCES.iter().any(|(prop, _, _)| it.props.contains_key(*prop));
    let mut out = Vec::new();
    let mut local: Vec<&str> = Vec::new();
    if armour_piece {
        let base_name = step.targets.iter().find(|(s, _, _)| *s == slot).map(|(_, b, _)| b.as_str()).unwrap_or_default();
        let base = base_defences(base_name).unwrap_or_default();
        let targets: Vec<&crate::guide::Target> = step.targets_for(slot).collect();
        for (i, (prop, flat, incs)) in ARMOUR_DEFENCES.iter().enumerate() {
            let flat_t: f64 = targets.iter().filter(|t| t.template == *flat).map(|t| t.value).sum();
            let inc_t: f64 = targets.iter().filter(|t| incs.contains(&t.template.as_str())).map(|t| t.value).sum();
            if flat_t == 0.0 && inc_t == 0.0 {
                continue;
            }
            local.push(flat);
            local.extend(incs.iter().copied());
            let expected = ((base[i] + flat_t) * (1.0 + inc_t / 100.0)).round();
            let have = it.props.get(*prop).copied().unwrap_or(0.0);
            let state = if have + 1e-6 >= expected * 0.95 {
                Have::Full(have)
            } else if have > 0.0 {
                Have::Partial(have)
            } else {
                Have::Missing
            };
            out.push((format!("≈ {expected} {prop} au total"), state));
        }
    }
    out.extend(step.targets_for(slot).filter(|t| !local.contains(&t.template.as_str())).map(|t| {
            let have = match values.get(&t.template) {
                Some(v) if *v + 1e-6 >= t.value => Have::Full(*v),
                Some(v) => Have::Partial(*v),
                None => Have::Missing,
            };
        (t.line.clone(), have)
    }));
    out
}

/// Intérêt pour le build de stats posées sur `slots` (modèle -> valeur) : 0 si le build ne
/// vise rien sur ces emplacements ou ne veut pas de ces stats.
pub fn build_value(step: &Step, slots: &[Slot], values: &BTreeMap<String, f64>) -> f64 {
    Profile::new(step, slots, &[], false).map(|p| p.score(values)).unwrap_or(0.0)
}

fn compare_for_build(it: &Item, refs: &[&Item], profile: &Profile) -> (Verdict, Vec<Delta>) {
    let new = mod_values([it]);
    let old = mod_values(refs.iter().copied());
    let mut diff = profile.score(&new) - profile.score(&old);
    let mut rows: Vec<(f64, Delta)> = Vec::new();
    for t in new.keys().chain(old.keys()).collect::<std::collections::BTreeSet<_>>() {
        let d = new.get(t).unwrap_or(&0.0) - old.get(t).unwrap_or(&0.0);
        if d.abs() < 1e-9 {
            continue;
        }
        match profile.weight(t) {
            Some((w, target)) => rows.push((w * (d / target).abs(), delta(render(t, d), d.signum() as i8))),
            // Hors build : neutre (sign 0), affiché en retrait.
            None => rows.push((0.0, delta(render(t, d), 0))),
        }
    }
    // Défenses de base : comptées si le build les recherche, en écart relatif.
    for (prop, label) in DEFENCES {
        let n = it.props.get(*prop).copied().unwrap_or(0.0);
        let o: f64 = refs.iter().map(|r| r.props.get(*prop).copied().unwrap_or(0.0)).sum();
        let d = n - o;
        if d.abs() < 1e-9 {
            continue;
        }
        if profile.defences.contains(prop) {
            let rel = d / n.max(o).max(1.0);
            diff += 0.5 * rel;
            rows.push((0.5 * rel.abs(), delta(format!("{}{} {label}", if d > 0.0 { "+" } else { "−" }, fmt_num(d.abs())), d.signum() as i8)));
        } else {
            rows.push((0.0, delta(format!("{}{} {label}", if d > 0.0 { "+" } else { "−" }, fmt_num(d.abs())), 0)));
        }
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let verdict = if diff > 0.25 {
        Verdict::Better
    } else if diff < -0.25 {
        Verdict::Worse
    } else {
        Verdict::Equal
    };
    (verdict, rows.into_iter().map(|(_, d)| d).collect())
}

// --- La base convient-elle au build ? ------------------------------------------------

/// Adéquation du type de base de la pièce à celui que vise le build.
#[derive(Clone, Debug, PartialEq)]
enum Fit {
    Ok,
    /// Utilisable, mais moins bien (base hybride quand le build en veut une pure…).
    Partial(String),
    /// Pas pour ce build, quels que soient ses mods.
    Off(String),
}

/// Nom français d'une classe d'objet, avec son article indéfini (« une baguette »).
fn class_label(class: &str) -> (&str, &str) {
    match class {
        "Staff" => ("un", "bâton"),
        "Warstaff" => ("un", "quarterstaff"),
        "Wand" => ("une", "baguette"),
        "Sceptre" => ("un", "sceptre"),
        "Focus" => ("un", "focus"),
        "Shield" => ("un", "bouclier"),
        "Buckler" => ("une", "targe"),
        "Bow" => ("un", "arc"),
        "Crossbow" => ("une", "arbalète"),
        "Quiver" => ("un", "carquois"),
        "Spear" => ("une", "lance"),
        "Talisman" => ("un", "talisman"),
        "Dagger" => ("une", "dague"),
        "Claw" => ("une", "griffe"),
        "Flail" => ("un", "fléau"),
        "One Hand Mace" => ("une", "masse à une main"),
        "Two Hand Mace" => ("une", "masse à deux mains"),
        "One Hand Sword" => ("une", "épée à une main"),
        "Two Hand Sword" => ("une", "épée à deux mains"),
        "One Hand Axe" => ("une", "hache à une main"),
        "Two Hand Axe" => ("une", "hache à deux mains"),
        c => ("un", c),
    }
}

fn element_label(element: &str) -> &str {
    match element {
        "fire" => "feu",
        "cold" => "froid",
        "lightning" => "foudre",
        "chaos" => "chaos",
        "physical" => "physique",
        e => e,
    }
}

const DEFENCE_LABELS: [&str; 3] = ["armure", "évasion", "bouclier d'énergie"];

fn defence_kinds(d: &[f64; 3]) -> Vec<usize> {
    (0..3).filter(|i| d[*i] > 0.0).collect()
}

fn defence_text(kinds: &[usize]) -> String {
    kinds.iter().map(|i| DEFENCE_LABELS[*i]).collect::<Vec<_>>().join(" / ")
}

/// Classe d'arme, élément d'un bâton ou d'une baguette, type de défense d'une pièce d'armure.
fn base_fit(have: &BaseMeta, want: &BaseMeta, want_name: &str) -> Fit {
    let (want_article, want_class) = class_label(&want.class);
    if have.class != want.class {
        let (article, class) = class_label(&have.class);
        return Fit::Off(format!("le build vise {want_article} {want_class} ({want_name}), pas {article} {class}"));
    }
    if let (Some(h), Some(w)) = (&have.element, &want.element) {
        if h != w {
            return Fit::Off(format!(
                "{want_class} de {} : le build vise {want_article} {want_class} de {} ({want_name}), seul à avoir les mods de sorts de cet élément",
                element_label(h),
                element_label(w)
            ));
        }
    }
    let (h, w) = (defence_kinds(&have.defences), defence_kinds(&want.defences));
    if w.is_empty() || h == w {
        return Fit::Ok;
    }
    if !h.iter().any(|k| w.contains(k)) {
        return Fit::Off(format!("base {} : le build vise {} ({want_name})", defence_text(&h), defence_text(&w)));
    }
    Fit::Partial(format!("base {} : le build vise {} ({want_name})", defence_text(&h), defence_text(&w)))
}

/// Compare `it` à ce qui est suivi. `None` si l'objet ne se porte pas (devise…).
/// Avec un build (nom, étape), le jugement se fait par rapport à l'équipement qu'il vise.
/// `item_base` : le type de base de `it` (classe, défenses…) ; `bases` : celui d'un type nommé
/// (la base visée par le build).
pub fn compare(it: &Item, gear: &Gear, build: Option<(&str, &Step)>, item_base: Option<BaseMeta>, bases: &dyn Fn(&str) -> Option<BaseMeta>) -> Option<Comparison> {
    let kind = gear::kind_of(&it.item_class)?;
    if let Some((&slot, _)) = gear.slots.iter().find(|(_, t)| **t == it.raw) {
        return Some(Comparison {
            verdict: Verdict::Same,
            against: vec![slot],
            against_name: it.title(),
            deltas: vec![],
            notes: vec![format!("c'est ta pièce suivie ({})", slot.label().to_lowercase())],
            basis: String::new(),
        });
    }
    // Les armes se comparent à celles du set affiché.
    let set = gear.set;
    let worn = |s: Slot| gear.item(s).map(|i| (s, i));
    let refs: Vec<(Slot, Item)> = match kind {
        Kind::Single(s) => worn(s.in_set(set)).into_iter().collect(),
        Kind::TwoHanded => [Slot::Weapon.in_set(set), Slot::Offhand.in_set(set)].into_iter().filter_map(worn).collect(),
        Kind::Ring => RINGS
            .into_iter()
            .filter_map(worn)
            .min_by(|a, b| score(&a.1).total_cmp(&score(&b.1)))
            .into_iter()
            .collect(),
        Kind::Charm => {
            let worn_charms: Vec<(Slot, Item)> = CHARMS.into_iter().filter_map(worn).collect();
            match worn_charms.iter().find(|(_, c)| c.base == it.base) {
                Some(found) => vec![found.clone()],
                None if worn_charms.is_empty() => vec![],
                None => {
                    let names: Vec<String> = worn_charms.iter().map(|(_, c)| c.base.clone()).collect();
                    return Some(Comparison::unknown(
                        format!("nouveau type de charme (tu portes : {})", names.join(", ")),
                        worn_charms.iter().map(|(s, _)| *s).collect(),
                    ));
                }
            }
        }
    };
    let target = match kind {
        Kind::Single(s) => s.in_set(set),
        Kind::TwoHanded => Slot::Weapon.in_set(set),
        Kind::Ring => Slot::RingLeft,
        Kind::Charm => Slot::Charm1,
    };
    if refs.is_empty() && target == Slot::Offhand.in_set(set) && gear.two_handed() {
        return Some(Comparison::unknown(
            "ta main gauche est prise par ton arme à deux mains : il faudrait aussi changer d'arme".into(),
            vec![Slot::Weapon.in_set(set)],
        ));
    }
    if refs.is_empty() {
        return Some(Comparison::unknown(
            format!("rien de suivi en {} : Ctrl+Alt+C sur ta pièce pour l'enregistrer", target.label().to_lowercase()),
            vec![],
        ));
    }
    let against: Vec<Slot> = refs.iter().map(|(s, _)| *s).collect();
    let against_name = refs.iter().map(|(_, i)| i.title()).collect::<Vec<_>>().join(" + ");
    let mut notes = Vec::new();
    if it.rarity == Rarity::Unique || refs.iter().any(|(_, i)| i.rarity == Rarity::Unique) {
        notes.push("effets d'uniques non comptés".to_string());
    }

    let profile_slots: Vec<Slot> = match kind {
        Kind::Single(s) => vec![s.in_set(set)],
        Kind::TwoHanded => vec![Slot::Weapon.in_set(set), Slot::Offhand.in_set(set)],
        Kind::Ring => RINGS.to_vec(),
        Kind::Charm => vec![],
    };
    // Base visée par le build sur cet emplacement.
    let wanted_base: Option<(String, BaseMeta)> = build.and_then(|(_, step)| {
        let name = step.targets.iter().find(|(s, b, _)| profile_slots.contains(s) && !b.is_empty()).map(|(_, b, _)| b.clone())?;
        bases(&name).map(|m| (name, m))
    });
    let caster = profile_slots.contains(&Slot::Weapon.in_set(set)) && weapon_dps(it).is_none() && it.item_class != "Shields";
    let implicits = wanted_base.as_ref().map(|(_, m)| m.implicits.clone()).unwrap_or_default();
    let profile = build.and_then(|(name, step)| Profile::new(step, &profile_slots, &implicits, caster).map(|p| (name, step, p)));
    let mut basis = String::new();
    let (verdict, deltas) = if matches!(kind, Kind::Single(Slot::LifeFlask | Slot::ManaFlask)) {
        compare_flask(it, &refs[0].1)
    } else if kind == Kind::Charm {
        compare_charm(it, &refs[0].1)
    } else if let Some((name, step, profile)) = &profile {
        basis = format!("{name} — {}", step.label);
        let worn: Vec<&Item> = refs.iter().map(|(_, i)| i).collect();
        compare_for_build(it, &worn, profile)
    } else {
        let new = stats(it);
        let mut old = vec![0.0; new.len()];
        for (_, i) in &refs {
            for (o, v) in old.iter_mut().zip(stats(i)) {
                *o += v;
            }
        }
        let deltas = labels()
            .zip(new.iter().zip(&old))
            .filter(|(_, (n, o))| (*n - *o).abs() > 0.0)
            .map(|(label, (n, o))| {
                let d = n - o;
                let sep = if label.starts_with('%') { "" } else { " " };
                delta(format!("{}{}{sep}{label}", if d > 0.0 { "+" } else { "−" }, fmt_num(d.abs())), d.signum() as i8)
            })
            .collect();
        let gain = score_stats(&new) - score_stats(&old);
        let v = if gain > 2.0 {
            Verdict::Better
        } else if gain < -2.0 {
            Verdict::Worse
        } else {
            Verdict::Equal
        };
        (v, deltas)
    };
    // Armes d'attaque : les dégâts de l'arme passent avant tout le reste.
    let (mut verdict, mut deltas) = (verdict, deltas);
    let worn_items: Vec<&Item> = refs.iter().map(|(_, i)| i).collect();
    let (dps_verdict, dps_rows) = weapon_rows(it, &worn_items, damage_focus(build.map(|(_, s)| s)));
    if let Some(v) = dps_verdict {
        verdict = v;
    }
    deltas.splice(0..0, dps_rows);
    // Une base qui ne convient pas au build passe avant tout : jamais recommandée.
    if let (Some((name, want)), Some(have)) = (&wanted_base, &item_base) {
        match base_fit(have, want, name) {
            Fit::Off(why) => {
                verdict = Verdict::Worse;
                deltas.insert(0, Delta { text: format!("pas pour ce build : {why}"), sign: -1, headline: true });
            }
            Fit::Partial(why) => {
                if verdict == Verdict::Better {
                    verdict = Verdict::Equal;
                }
                deltas.insert(0, Delta { text: why, sign: -1, headline: true });
            }
            Fit::Ok => {}
        }
    }
    if build.is_some() && profile.is_none() && !matches!(kind, Kind::Charm | Kind::Single(Slot::LifeFlask | Slot::ManaFlask)) {
        notes.push("le build ne précise rien pour cet emplacement : jugement générique".into());
    }
    Some(Comparison { verdict, against, against_name, deltas, notes, basis })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{self, tests::{GLOOM_TOUCH, MANA_FLASK, STONE_CHARM}};
    use std::path::Path;

    const BETTER_GLOVES: &str = "Item Class: Gloves\nRarity: Rare\nStorm Grip\nTorn Gloves\n--------\nEnergy Shield: 40 (augmented)\n--------\nItem Level: 20\n--------\n+45 to maximum Life\n+18% to Fire Resistance\n+12% to Lightning Resistance";

    fn gear_with(items: &[&str]) -> Gear {
        let mut g = Gear::load(Path::new("/nonexistent/gear.json"));
        for t in items {
            g.equip(&item::parse(t).unwrap());
        }
        g
    }

    const MACE_LOW: &str = "Item Class: Two Hand Maces\nRarity: Magic\nHeavy Oak Greathammer\n--------\nPhysical Damage: 6-9 (augmented)\nCritical Hit Chance: 5.00%\nAttacks per Second: 1.10\n--------\nItem Level: 8\n--------\n+30 to maximum Life\n+20% to Fire Resistance";
    const MACE_HIGH: &str = "Item Class: Two Hand Maces\nRarity: Normal\nOak Greathammer\n--------\nPhysical Damage: 11-16\nCritical Hit Chance: 5.00%\nAttacks per Second: 1.10\n--------\nItem Level: 10";

    #[test]
    fn weapon_damage_comes_first() {
        let gear = gear_with(&[MACE_LOW]);
        let c = compare(&item::parse(MACE_HIGH).unwrap(), &gear, None, None, &|_| None).unwrap();
        assert_eq!(c.verdict, Verdict::Better, "{:?}", c.deltas.iter().map(|d| &d.text).collect::<Vec<_>>());
        assert!(c.deltas[0].text.starts_with("DPS 14,9 au lieu de 8,3"), "{}", c.deltas[0].text);
        assert!(c.deltas[1].text.contains("11-16 au lieu de 6-9"), "{}", c.deltas[1].text);
    }

    #[test]
    fn better_gloves() {
        let g = gear_with(&[GLOOM_TOUCH]);
        let c = compare(&item::parse(BETTER_GLOVES).unwrap(), &g, None, None, &|_| None).unwrap();
        assert_eq!(c.verdict, Verdict::Better);
        assert_eq!(c.against, vec![Slot::Gloves]);
        let texts: Vec<&str> = c.deltas.iter().map(|d| d.text.as_str()).collect();
        assert!(texts.contains(&"+24 vie"), "{texts:?}");
        assert!(texts.contains(&"+18% rés. feu"), "{texts:?}");
    }

    #[test]
    fn same_item_and_nothing_tracked() {
        let g = gear_with(&[GLOOM_TOUCH]);
        assert_eq!(compare(&item::parse(GLOOM_TOUCH).unwrap(), &g, None, None, &|_| None).unwrap().verdict, Verdict::Same);
        let empty = gear_with(&[]);
        assert_eq!(compare(&item::parse(GLOOM_TOUCH).unwrap(), &empty, None, None, &|_| None).unwrap().verdict, Verdict::Unknown);
    }

    #[test]
    fn judged_against_the_build_targets() {
        use crate::guide::{Step, Target};
        let target = |t: &str, v: f64| Target { template: t.into(), value: v, line: String::new() };
        let step = Step {
            label: "Act 3".into(),
            skills: vec![],
            passives: vec![],
            targets: vec![(Slot::Gloves, "Rope Cuffs".into(), vec![target("# to maximum Life", 59.0), target("#% to Fire Resistance", 10.0)])],
            uniques: vec![],
            passive_sets: Default::default(),
        };
        let g = gear_with(&[GLOOM_TOUCH]);
        // Gants avec beaucoup de précision et d'évasion mais sans vie : hors build.
        let accuracy = "Item Class: Gloves\nRarity: Rare\nA\nTorn Gloves\n--------\nItem Level: 20\n--------\n+150 to Accuracy Rating\n+40% to Fire Resistance";
        let c = compare(&item::parse(accuracy).unwrap(), &g, Some(("Ky", &step)), None, &|_| None).unwrap();
        assert_eq!(c.basis, "Ky — Act 3");
        assert!(c.deltas.iter().any(|d| d.text.contains("Accuracy") && d.sign == 0), "{:?}", c.deltas);
        // Perte de 21 vie (cible 59) contre +40 % feu plafonné à 1,5 × la cible : globalement mieux.
        assert_eq!(c.verdict, Verdict::Better);
        let c = compare(&item::parse(BETTER_GLOVES).unwrap(), &g, Some(("Ky", &step)), None, &|_| None).unwrap();
        assert_eq!(c.verdict, Verdict::Better);
    }

    #[test]
    fn targets_are_checked_line_by_line() {
        use crate::guide::{Step, Target};
        let target = |t: &str, v: f64, line: &str| Target { template: t.into(), value: v, line: line.into() };
        let step = Step {
            label: "Act 3".into(),
            skills: vec![],
            passives: vec![],
            targets: vec![(Slot::Gloves, "Rope Cuffs".into(), vec![
                target("# to maximum Life", 59.0, "+59 to maximum Life"),
                target("#% to Fire Resistance", 10.0, "+10% to Fire Resistance"),
                target("#% to Cold Resistance", 10.0, "+10% to Cold Resistance"),
            ])],
            uniques: vec![],
            passive_sets: Default::default(),
        };
        let gloves = item::parse("Item Class: Gloves\nRarity: Rare\nA\nTorn Gloves\n--------\nItem Level: 20\n--------\n+34 to maximum Life\n+15% to Fire Resistance").unwrap();
        let c = check_targets(&gloves, &step, Slot::Gloves, |_| None);
        assert_eq!(c.iter().map(|(_, h)| *h).collect::<Vec<_>>(), [Have::Partial(34.0), Have::Full(15.0), Have::Missing]);
    }

    #[test]
    fn armour_defences_are_compared_as_totals() {
        use crate::guide::{Step, Target};
        let target = |t: &str, v: f64| Target { template: t.into(), value: v, line: String::new() };
        let step = Step {
            label: "Endgame".into(),
            skills: vec![],
            passives: vec![],
            targets: vec![(Slot::Helmet, "Ancestral Tiara".into(), vec![
                target("# to maximum Energy Shield", 40.0),
                target("#% increased Energy Shield", 50.0),
                target("# to maximum Life", 80.0),
            ])],
            uniques: vec![],
            passive_sets: Default::default(),
        };
        let base = |n: &str| (n == "Ancestral Tiara").then_some([0.0, 0.0, 109.0]);
        // (109 + 40) × 1,5 ≈ 224 visés ; la pièce a 230 sans aucun mod de bouclier d'énergie.
        let helm = item::parse("Item Class: Helmets\nRarity: Rare\nA\nTiara\n--------\nEnergy Shield: 230 (augmented)\n--------\nItem Level: 80\n--------\n+80 to maximum Life").unwrap();
        let c = check_targets(&helm, &step, Slot::Helmet, base);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], ("≈ 224 Energy Shield au total".to_string(), Have::Full(230.0)));
        assert_eq!(c[1].1, Have::Full(80.0));
        let weak = item::parse(&"Item Class: Helmets\nRarity: Rare\nA\nTiara\n--------\nEnergy Shield: 120 (augmented)\n--------\nItem Level: 80").unwrap();
        assert_eq!(check_targets(&weak, &step, Slot::Helmet, base)[0].1, Have::Partial(120.0));
    }

    #[test]
    fn flasks_and_charms() {
        let g = gear_with(&[MANA_FLASK, STONE_CHARM]);
        let bigger = MANA_FLASK.replace("Recovers 90 Mana", "Recovers 150 Mana");
        assert_eq!(compare(&item::parse(&bigger).unwrap(), &g, None, None, &|_| None).unwrap().verdict, Verdict::Better);
        let longer = STONE_CHARM.replace("Lasts 3", "Lasts 5");
        let c = compare(&item::parse(&longer).unwrap(), &g, None, None, &|_| None).unwrap();
        assert_eq!((c.verdict, c.against.clone()), (Verdict::Better, vec![Slot::Charm1]));
        // La quantité rendue l'emporte sur les charges ; à quantité égale, les charges départagent.
        let bigger_fewer = MANA_FLASK.replace("Recovers 90 Mana", "Recovers 130 Mana").replace("10 of 70", "20 of 40");
        let c = compare(&item::parse(&bigger_fewer).unwrap(), &g, None, None, &|_| None).unwrap();
        assert_eq!(c.verdict, Verdict::Better);
        assert!(c.deltas[0].text.starts_with("rend 130 au lieu de 90"));
        let fewer = MANA_FLASK.replace("10 of 70", "20 of 40");
        assert_eq!(compare(&item::parse(&fewer).unwrap(), &g, None, None, &|_| None).unwrap().verdict, Verdict::Worse);
        let other = STONE_CHARM.replace("Stone Charm", "Thawing Charm");
        assert_eq!(compare(&item::parse(&other).unwrap(), &g, None, None, &|_| None).unwrap().verdict, Verdict::Unknown);
    }

    fn meta(class: &str, element: Option<&str>, defences: [f64; 3]) -> BaseMeta {
        BaseMeta { class: class.into(), element: element.map(Into::into), defences, implicits: vec![] }
    }

    #[test]
    fn base_must_suit_the_build() {
        let es = meta("Helmet", None, [0.0, 0.0, 50.0]);
        assert_eq!(base_fit(&es, &es, "Tiara"), Fit::Ok);
        assert!(matches!(base_fit(&meta("Helmet", None, [80.0, 0.0, 0.0]), &es, "Tiara"), Fit::Off(_)));
        assert!(matches!(base_fit(&meta("Helmet", None, [40.0, 0.0, 25.0]), &es, "Tiara"), Fit::Partial(_)));
        let cold = meta("Staff", Some("cold"), [0.0; 3]);
        assert!(matches!(base_fit(&meta("Staff", Some("lightning"), [0.0; 3]), &cold, "Gelid Staff"), Fit::Off(_)));
        assert!(matches!(base_fit(&meta("Wand", None, [0.0; 3]), &cold, "Gelid Staff"), Fit::Off(_)));
    }

    /// Une pièce d'armure n'est jamais recommandée sur un build bouclier d'énergie, même avec
    /// de meilleurs mods.
    #[test]
    fn wrong_defence_type_is_worse() {
        let g = gear_with(&[GLOOM_TOUCH]);
        let step = crate::guide::Step {
            label: "x".into(),
            skills: vec![],
            passives: vec![],
            targets: vec![(Slot::Gloves, "Silk Gloves".into(), vec![crate::guide::Target { template: "# to maximum Life".into(), value: 40.0, line: "+40 to maximum Life".into() }])],
            uniques: vec![],
            passive_sets: Default::default(),
        };
        let bases = |n: &str| (n == "Silk Gloves").then(|| meta("Gloves", None, [0.0, 0.0, 30.0]));
        let armour = Some(meta("Gloves", None, [60.0, 0.0, 0.0]));
        let c = compare(&item::parse(BETTER_GLOVES).unwrap(), &g, Some(("Ky", &step)), armour, &bases).unwrap();
        assert_eq!(c.verdict, Verdict::Worse);
        assert!(c.deltas[0].text.starts_with("pas pour ce build"), "{:?}", c.deltas);
        let es = Some(meta("Gloves", None, [0.0, 0.0, 30.0]));
        let c = compare(&item::parse(BETTER_GLOVES).unwrap(), &g, Some(("Ky", &step)), es, &bases).unwrap();
        assert_ne!(c.verdict, Verdict::Worse);
    }

    #[test]
    fn implicit_ranges_become_templates() {
        assert_eq!(implicit_template("+(20-30)% to Cold Resistance"), ("#% to Cold Resistance".into(), Some(25.0)));
    }

    #[test]
    fn caster_core_lines_weigh_more() {
        let step = crate::guide::Step {
            label: "x".into(),
            skills: vec![],
            passives: vec![],
            targets: vec![(
                Slot::Weapon,
                String::new(),
                vec![
                    crate::guide::Target { template: "#% increased Spell Damage".into(), value: 100.0, line: String::new() },
                    crate::guide::Target { template: "# to maximum Mana".into(), value: 100.0, line: String::new() },
                ],
            )],
            uniques: vec![],
            passive_sets: Default::default(),
        };
        let p = Profile::new(&step, &[Slot::Weapon], &[], true).unwrap();
        assert!(p.weight("#% increased Spell Damage").unwrap().0 > p.weight("# to maximum Mana").unwrap().0);
    }
}
