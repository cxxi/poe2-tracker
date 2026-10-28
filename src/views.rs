//! Onglets du build : sorts, talents (arbre), ascendances et uniques, pour une étape du guide.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::art::Assets;
use crate::gem_info::GemInfo;
use crate::guide::{Catalog, Step};
use crate::prices::PriceData;
use crate::ui::{GOLD, GOLD_DIM};
use crate::uniques;

const GREEN: Color32 = Color32::from_rgb(110, 210, 110);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Equipment,
    Stats,
    Skills,
    Passives,
    Ascendancy,
    Uniques,
}

/// Onglets du panneau de droite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideTab {
    Diagnostic,
    Market,
}

impl SideTab {
    pub const ALL: [SideTab; 2] = [SideTab::Diagnostic, SideTab::Market];

    pub fn label(self) -> &'static str {
        match self {
            SideTab::Diagnostic => "Diagnostic",
            SideTab::Market => "Marché",
        }
    }
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Equipment,
        Tab::Stats, Tab::Skills, Tab::Passives, Tab::Ascendancy, Tab::Uniques];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Equipment => "Équipement",
            Tab::Stats => "Caractéristiques",
            Tab::Skills => "Sorts",
            Tab::Passives => "Talents",
            Tab::Ascendancy => "Ascendances",
            Tab::Uniques => "Uniques",
        }
    }
}

/// Choix de l'étape affichée ; `None` = étape actuelle du personnage.
pub fn step_selector(ui: &mut egui::Ui, steps: &[Step], selected: &mut Option<usize>, current: Option<usize>) -> usize {
    let shown = selected.or(current).unwrap_or(0).min(steps.len().saturating_sub(1));
    let label = |i: usize| {
        let s = &steps[i].label;
        if Some(i) == current { format!("{s} (actuelle)") } else { s.clone() }
    };
    egui::ComboBox::from_id_salt("view_step").selected_text(RichText::new(label(shown)).color(GOLD)).show_ui(ui, |ui| {
        for i in 0..steps.len() {
            if ui.selectable_label(i == shown, label(i)).clicked() {
                *selected = if Some(i) == current { None } else { Some(i) };
            }
        }
    });
    shown
}

// --- Sorts ----------------------------------------------------------------------------

/// Infobulle d'une gemme : description et effets (RePoE), quand on les connaît.
/// Gemmes qui ne se taillent pas (lignée…) : couleur des objets uniques.
const SPECIAL_GEM: Color32 = Color32::from_rgb(210, 125, 60);

/// « Uncut Skill Gem · niv. 9 » : la gemme non taillée dont on tire cette gemme.
fn uncut_text(info: &GemInfo) -> String {
    match info.uncut_level {
        0 => format!("Uncut {} Gem", info.uncut),
        n => format!("Uncut {} Gem · niv. {n}", info.uncut),
    }
}

fn gem_tooltip(ui: &mut egui::Ui, name: &str, note: &str, info: Option<&GemInfo>) {
    ui.set_max_width(380.0);
    ui.label(RichText::new(name).color(GOLD).strong());
    if !note.is_empty() {
        ui.label(RichText::new(note).color(Color32::GRAY).small());
    }
    if let Some(i) = info {
        let origin = match (i.lineage, i.special()) {
            (true, _) => "Gemme de lignée : se trouve telle quelle, ne se taille pas".to_string(),
            (false, true) => "Gemme spéciale : ne se taille pas dans une Uncut Support Gem".to_string(),
            _ => format!("À tailler dans : {}", uncut_text(i)),
        };
        ui.label(RichText::new(origin).color(if i.special() { SPECIAL_GEM } else { Color32::GRAY }).small());
        if !i.description.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(&i.description).color(Color32::LIGHT_GRAY));
        }
        if !i.effects.is_empty() {
            ui.add_space(4.0);
            for e in &i.effects {
                ui.label(RichText::new(e).color(Color32::from_rgb(136, 136, 255)));
            }
        }
    }
}

pub fn skills(ui: &mut egui::Ui, step: &Step, prev: Option<&Step>, level: u32, assets: &mut Assets, infos: Option<&HashMap<String, GemInfo>>) {
    let before: HashSet<&str> = prev.map(|p| p.skills.iter().map(|s| s.name.as_str()).collect()).unwrap_or_default();
    // Supports sur une ligne si ceux de chaque sort tiennent dans la largeur, sinon les uns sous
    // les autres pour tous les sorts : jamais un mélange des deux.
    let font = egui::TextStyle::Body.resolve(ui.style());
    let spacing = ui.spacing().item_spacing.x;
    let row_width = |supports: &[(String, u32)]| -> f32 {
        // Icône, nom, étiquette « niv. N » (environ 40 px) et espacements.
        supports.iter().map(|(n, _)| 22.0 + 6.0 + ui.fonts(|f| f.layout_no_wrap(n.clone(), font.clone(), Color32::WHITE).size().x) + 40.0 + 3.0 * spacing).sum()
    };
    // Largeur des supports dans une carte : moins les marges du cadre et l'icône du sort.
    let room = ui.available_width() - 16.0 - 44.0 - 3.0 * spacing;
    let horizontal = step.skills.iter().all(|s| row_width(&s.supports) <= room);
    for skill in &step.skills {
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(10, 9, 8, 210))
            .stroke(Stroke::new(1.0_f32, GOLD_DIM))
            .corner_radius(4)
            .inner_margin(8)
            .outer_margin(egui::Margin::symmetric(0, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let info = infos.and_then(|m| m.get(&skill.name));
                ui.horizontal(|ui| {
                    icon(ui, assets, &skill.name, 44.0).on_hover_ui(|ui| gem_tooltip(ui, &skill.name, "", info));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&skill.name).color(GOLD).size(16.0).strong()).on_hover_ui(|ui| gem_tooltip(ui, &skill.name, "", info));
                            if prev.is_some() && !before.contains(skill.name.as_str()) {
                                ui.label(RichText::new("nouveau").color(GREEN).small().strong());
                            }
                            if let Some(set) = skill.weapon_set {
                                set_badge(ui, set);
                            }
                        });
                        let (txt, col) = if skill.from_level > level {
                            (format!("à partir du niveau {} (tu es niveau {level})", skill.from_level), Color32::from_rgb(220, 150, 90))
                        } else {
                            (format!("dès le niveau {}", skill.from_level), Color32::GRAY)
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(txt).color(col).small());
                            if let Some(i) = info {
                                ui.label(RichText::new(format!("· {}", uncut_text(i))).color(Color32::GRAY).small());
                            }
                        });
                        let layout = if horizontal { egui::Layout::left_to_right(egui::Align::Center) } else { egui::Layout::top_down(egui::Align::Min) };
                        ui.with_layout(layout, |ui| {
                            for (name, lvl) in &skill.supports {
                                let info = infos.and_then(|m| m.get(name));
                                let note = format!("support dès le niveau {lvl}");
                                ui.horizontal(|ui| {
                                    icon(ui, assets, name, 22.0).on_hover_ui(|ui| gem_tooltip(ui, name, &note, info));
                                    // Gemme spéciale (lignée…) : en couleur, pour la repérer.
                                    let special = info.is_some_and(|i| i.special());
                                    let mut t = RichText::new(name).color(if special { SPECIAL_GEM } else { Color32::LIGHT_GRAY });
                                    if *lvl > level {
                                        t = t.color(Color32::GRAY).italics();
                                    }
                                    ui.label(t).on_hover_ui(|ui| gem_tooltip(ui, name, &note, info));
                                    // Niveau de l'Uncut Support Gem à tailler.
                                    if let Some(n) = info.map(|i| i.uncut_level).filter(|n| *n > 0) {
                                        ui.label(RichText::new(format!("niv. {n}")).color(Color32::GRAY).small())
                                            .on_hover_text(format!("Uncut Support Gem de niveau {n} minimum"));
                                    }
                                    ui.add_space(6.0);
                                });
                            }
                        });
                    });
                });
            });
    }
    if let Some(prev) = prev {
        let now: HashSet<&str> = step.skills.iter().map(|s| s.name.as_str()).collect();
        let dropped: Vec<&str> = prev.skills.iter().map(|s| s.name.as_str()).filter(|n| !now.contains(n)).collect();
        if !dropped.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Abandonnés à cette étape : {}", dropped.join(", "))).color(Color32::GRAY));
        }
    }
}

/// Set d'armes où placer un sort, dans la couleur du set (celle de l'onglet Talents).
fn set_badge(ui: &mut egui::Ui, set: u8) {
    let (text, color, hint) = match set {
        1 | 2 => {
            let roman = if set == 1 { "I" } else { "II" };
            (format!("Set {roman}"), crate::ui::SET_COLORS[set as usize - 1], format!("À placer dans le set d'armes {roman} : le sort ne s'utilise qu'avec ce set, et profite de ses talents."))
        }
        _ => ("Sets I et II".to_string(), GOLD_DIM, "À placer dans les deux sets d'armes.".to_string()),
    };
    egui::Frame::new()
        .stroke(Stroke::new(1.0_f32, color))
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| ui.label(RichText::new(text).color(color).small().strong()))
        .response
        .on_hover_text(hint);
}

fn icon(ui: &mut egui::Ui, assets: &mut Assets, name: &str, size: f32) -> egui::Response {
    match assets.texture_by_name(name) {
        Some(tex) => {
            let s = tex.size_vec2();
            let k = (size / s.x).min(size / s.y);
            ui.add_sized([size, size], egui::Image::new((tex.id(), s * k)).sense(Sense::hover()))
        }
        None => ui.allocate_exact_size(Vec2::splat(size), Sense::hover()).1,
    }
}

// --- Uniques -------------------------------------------------------------------------

const UNIQUE: Color32 = Color32::from_rgb(175, 96, 37);

/// Uniques de l'étape affichée (en cartes), puis tous ceux du build, les plus partagés
/// entre étapes d'abord.
pub fn uniques(ui: &mut egui::Ui, steps: &[Step], shown: usize, worn: &HashSet<String>, prices: Option<&PriceData>, league: &str, assets: &mut Assets) {
    let summary = uniques::summary(steps);
    let step = &steps[shown];
    let price = |name: &str| -> (Option<String>, String) {
        let Some((base, div, listings)) = prices.and_then(|p| p.unique_by_name(name)) else { return (None, "prix ?".into()) };
        let p = prices.unwrap();
        let text = if listings < crate::prices::MIN_LISTINGS { format!("~{} (peu d'annonces)", p.fmt(div)) } else { format!("≈ {}", p.fmt(div)) };
        (Some(base.to_string()), text)
    };
    let here = uniques::of_step(step);
    if here.is_empty() {
        ui.label(RichText::new(format!("Aucun unique recommandé à l'étape {}.", step.label)).color(Color32::GRAY));
    }
    for (w, count) in here {
        let (base, price_text) = price(&w.name);
        let others: Vec<&str> = summary
            .iter()
            .find(|s| s.name == w.name)
            .map(|s| s.steps.iter().map(String::as_str).filter(|l| *l != step.label).collect())
            .unwrap_or_default();
        let have = worn.contains(&w.name);
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(10, 9, 8, 210))
            .stroke(Stroke::new(1.0_f32, if have { GREEN.gamma_multiply(0.6) } else { GOLD_DIM }))
            .corner_radius(4)
            .inner_margin(8)
            .outer_margin(egui::Margin::symmetric(0, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    unique_icon(ui, assets, &w.name, 44.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            let title = if count > 1 { format!("{} ×{count}", w.name) } else { w.name.clone() };
                            ui.label(RichText::new(title).color(UNIQUE).size(16.0).strong());
                            if have {
                                ui.label(RichText::new("✓ porté").color(GREEN).strong());
                            }
                        });
                        let slot = uniques::slot_label(uniques::slot(w.slot, base.as_deref()));
                        let what = match &base {
                            Some(b) => format!("{slot} · {b}"),
                            None => slot.to_string(),
                        };
                        ui.label(RichText::new(what).color(Color32::GRAY).small());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&price_text).color(GOLD));
                            ui.hyperlink_to("chercher sur le trade ↗", uniques::trade_url(league, &w.name, base.as_deref()));
                        });
                        if !others.is_empty() {
                            ui.label(RichText::new(format!("aussi dans : {}", others.join(", "))).color(Color32::GRAY).small());
                        }
                    });
                });
            });
    }
    if summary.is_empty() {
        return;
    }
    ui.add_space(10.0);
    ui.label(RichText::new("TOUT LE BUILD").color(GOLD_DIM).strong());
    ui.label(RichText::new("du plus utilisé entre étapes au moins utilisé").color(Color32::DARK_GRAY).small());
    ui.add_space(4.0);
    egui::Grid::new("all_uniques").num_columns(4).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
        for s in &summary {
            let (base, price_text) = price(&s.name);
            let have = worn.contains(&s.name);
            ui.horizontal(|ui| {
                ui.label(RichText::new(if have { "✓" } else { "★" }).color(if have { GREEN } else { UNIQUE }));
                ui.hyperlink_to(RichText::new(&s.name).color(UNIQUE), uniques::trade_url(league, &s.name, base.as_deref()))
                    .on_hover_text("chercher sur le trade");
            });
            ui.label(RichText::new(uniques::slot_label(uniques::slot(s.slot, base.as_deref()))).color(Color32::GRAY));
            let n = s.steps.len();
            ui.label(RichText::new(if n > 1 { format!("{n} étapes") } else { s.steps[0].clone() }).color(Color32::LIGHT_GRAY))
                .on_hover_text(s.steps.join(", "));
            ui.label(RichText::new(price_text).color(GOLD));
            ui.end_row();
        }
    });
}

fn unique_icon(ui: &mut egui::Ui, assets: &mut Assets, name: &str, size: f32) {
    match assets.texture_unique(name) {
        Some(tex) => {
            let s = tex.size_vec2();
            let k = (size / s.x).min(size / s.y);
            ui.add_sized([size, size], egui::Image::new((tex.id(), s * k)));
        }
        None => {
            ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        }
    }
}

// --- Ascendances ----------------------------------------------------------------------

pub fn ascendancy(ui: &mut egui::Ui, steps: &[Step], current: Option<usize>, catalog: &Catalog) {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut any = false;
    for (i, step) in steps.iter().enumerate() {
        let fresh: Vec<&crate::guide::Passive> = step
            .passives
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .filter_map(|id| catalog.passives.get(id))
            .filter(|p| p.ascendancy && p.notable)
            .collect();
        if fresh.is_empty() {
            continue;
        }
        any = true;
        let reached = current.is_some_and(|c| i <= c);
        ui.add_space(6.0);
        ui.label(RichText::new(&step.label).color(if reached { GOLD } else { GOLD_DIM }).size(13.0).strong());
        for p in fresh {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(10, 9, 8, 210))
                .stroke(Stroke::new(1.0_f32, if reached { GOLD } else { GOLD_DIM }))
                .corner_radius(4)
                .inner_margin(8)
                .outer_margin(egui::Margin::symmetric(0, 3))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&p.name).color(GOLD).size(15.0).strong());
                    for s in &p.stats {
                        ui.label(RichText::new(s).color(Color32::from_rgb(136, 136, 255)));
                    }
                });
        }
    }
    if !any {
        ui.label(RichText::new("Ce build ne précise pas d'ascendance.").color(Color32::GRAY));
    }
}

// --- Talents (arbre) -----------------------------------------------------------------

struct Node {
    id: String,
    pos: Pos2,
    kind: u8, // 0 petit, 1 notable, 2 keystone, 3 joyau, 4 départ de classe
    /// Nœuds reliés, avec le rayon d'arc signé de la liaison (0 = droite).
    links: Vec<(usize, f32)>,
    icon: String,
    group: Pos2,
    orbit: u8,
    attribute: bool,
}

/// Les trois attributs : nom, couleur, icône « + » du jeu.
const ATTRIBUTES: [(&str, Color32, &str); 3] = [
    ("Force", Color32::from_rgb(205, 75, 60), "Art/2DArt/SkillIcons/passives/plusstrength.dds"),
    ("Dextérité", Color32::from_rgb(95, 185, 85), "Art/2DArt/SkillIcons/passives/plusdexterity.dds"),
    ("Intelligence", Color32::from_rgb(85, 135, 235), "Art/2DArt/SkillIcons/passives/plusintelligence.dds"),
];

/// Points d'un arc de cercle de `a` à `b` autour de `center`, par le plus court chemin.
fn arc(a: Pos2, b: Pos2, center: Pos2, segments: usize) -> Vec<Pos2> {
    let (ra, rb) = ((a - center).angle(), (b - center).angle());
    let mut d = rb - ra;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    let r = (a - center).length();
    (0..=segments).map(|i| center + Vec2::angled(ra + d * i as f32 / segments as f32) * r).collect()
}

/// Tracé d'une liaison (coordonnées de l'arbre) : arc de rayon donné, arc du groupe, ou droite.
fn link_path(a: &Node, b: &Node, radius: f32) -> Vec<Pos2> {
    let (pa, pb) = (a.pos, b.pos);
    if radius != 0.0 {
        let half = (pb - pa).length() / 2.0;
        let r = radius.abs();
        if r > half && half > 0.0 {
            let mid = pa + (pb - pa) / 2.0;
            let normal = Vec2::new(-(pb - pa).y, (pb - pa).x) / (2.0 * half);
            // Convention vérifiée sur le rendu : côté opposé au signe de la liaison.
            let center = mid - normal * (r * r - half * half).sqrt() * radius.signum();
            return arc(pa, pb, center, 16);
        }
    }
    if a.group == b.group && a.orbit == b.orbit && a.orbit > 0 {
        return arc(pa, pb, a.group, 16);
    }
    vec![pa, pb]
}

const CENTER_CIRCLE: &str = "art/textures/interface/2d/2dart/uiimages/ingame/passivetree/passivetreemaincircle.dds";
const ACTIVE_START: &str = "art/textures/interface/2d/2dart/uiimages/ingame/passivetree/passivetreemaincircleactive.dds";
/// Distance des départs rapportée à la demi-largeur du médaillon : la croix tombe dans son trèfle.
const RING_FRACTION: f32 = 0.784;
/// Agrandissement du médaillon (et de la croix, repoussée d'autant) pour que les premiers
/// nœuds, posés à leur vraie position, tombent dans les alvéoles de l'ornement comme en jeu.
const MEDALLION_SCALE: f32 = 1.055;
/// Largeur de l'image du départ actif rapportée à celle du médaillon (364 px sur 4008 px).
const START_FRACTION: f32 = 364.0 / 4008.0;
/// Position de la croix dans l'image du départ actif (mesurée) : elle n'est pas centrée.
const START_ANCHOR: Vec2 = Vec2::new(0.494, 0.603);

/// Rayon d'un nœud en unités de l'arbre (PoB), selon son type.
fn world_radius(kind: u8) -> f32 {
    match kind {
        2 => 95.0,
        1 => 70.0,
        3 => 60.0,
        4 => 80.0,
        _ => 45.0,
    }
}

#[derive(Default)]
pub struct TreeView {
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
    center: Pos2,
    zoom: f32,
    pan: Vec2,
    fitted_for: Option<String>,
    /// Attribut choisi à la main pour un nœud d'attribut (0 Force, 1 Dextérité, 2 Intelligence),
    /// mémorisé par build dans `overrides_file`.
    overrides: HashMap<String, usize>,
    overrides_file: Option<std::path::PathBuf>,
}

impl TreeView {
    fn build(&mut self, catalog: &Catalog) {
        if !self.nodes.is_empty() || catalog.passives.is_empty() {
            return;
        }
        let main: Vec<(&String, &crate::guide::Passive)> =
            catalog.passives.iter().filter(|(_, p)| !p.ascendancy && !p.hidden && (p.x != 0.0 || p.y != 0.0)).collect();
        let by_skill: HashMap<u32, usize> = main.iter().enumerate().map(|(i, (_, p))| (p.skill, i)).collect();
        self.nodes = main
            .iter()
            .map(|(id, p)| Node {
                id: (*id).clone(),
                pos: Pos2::new(p.x, p.y),
                kind: if p.class_start { 4 } else if p.keystone { 2 } else if p.jewel { 3 } else if p.notable { 1 } else { 0 },
                links: p
                    .links
                    .iter()
                    .enumerate()
                    .filter_map(|(k, s)| by_skill.get(s).map(|j| (*j, p.link_arcs.get(k).copied().unwrap_or(0.0))))
                    .collect(),
                icon: p.icon.clone(),
                group: Pos2::new(p.gx, p.gy),
                orbit: p.orbit,
                attribute: p.attribute,
            })
            .collect();
        self.index = self.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
    }

    /// Fichier des choix d'attributs du build affiché (rechargé quand le build change).
    pub fn use_overrides_file(&mut self, path: std::path::PathBuf) {
        if self.overrides_file.as_ref() == Some(&path) {
            return;
        }
        self.overrides = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        self.overrides_file = Some(path);
    }

    fn save_overrides(&self) {
        if let Some(path) = &self.overrides_file {
            let _ = std::fs::write(path, serde_json::to_string_pretty(&self.overrides).unwrap_or_default());
        }
    }

    /// Cadre la vue sur les nœuds du build (ou tout l'arbre), avec un minimum de contexte.
    fn fit(&mut self, rect: Rect, ids: &HashSet<usize>) {
        let pts: Vec<Pos2> = if ids.is_empty() { self.nodes.iter().map(|n| n.pos).collect() } else { ids.iter().map(|i| self.nodes[*i].pos).collect() };
        if pts.is_empty() {
            return;
        }
        let bb = Rect::from_points(&pts).expand(600.0);
        let size = bb.size().max(Vec2::splat(5000.0));
        self.center = bb.center();
        self.zoom = (rect.width() / size.x).min(rect.height() / size.y) * 0.95;
        self.pan = Vec2::ZERO;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        step: &Step,
        prev: Option<&Step>,
        catalog: &Catalog,
        icons: &mut crate::passive_icons::PassiveIcons,
        key: String,
        set: u8,
    ) {
        self.build(catalog);
        if self.nodes.is_empty() {
            ui.put(rect, egui::Label::new(RichText::new("Arbre en cours de chargement…").color(Color32::GRAY)));
            return;
        }
        // Passifs d'un set d'armes : ceux du set affiché comptent comme pris, ceux de l'autre
        // set restent éteints, juste entourés de leur couleur.
        let taken_in = |s: &Step| -> HashSet<usize> {
            s.passives
                .iter()
                .filter(|id| s.passive_sets.get(*id).is_none_or(|n| *n == set))
                .filter_map(|id| self.index.get(id).copied())
                .collect()
        };
        let mut taken = taken_in(step);
        let before: HashSet<usize> = prev.map(taken_in).unwrap_or_default();
        let set_of: HashMap<usize, u8> = step.passive_sets.iter().filter_map(|(id, n)| Some((*self.index.get(id)?, *n))).collect();
        let per_set = [1u8, 2].map(|n| set_of.values().filter(|v| **v == n).count());
        let counted = taken.len();
        let fresh_count = if prev.is_some() { taken.difference(&before).count() } else { 0 };
        // Le départ de la classe est pris d'office : on le relie au chemin du build.
        let starts: Vec<usize> = (0..self.nodes.len())
            .filter(|i| self.nodes[*i].kind == 4 && self.nodes[*i].links.iter().any(|(j, _)| taken.contains(j)))
            .collect();
        taken.extend(starts);
        if self.fitted_for.as_ref() != Some(&key) {
            self.fit(rect, &taken);
            self.fitted_for = Some(key);
        }

        // Déplacement à la souris, zoom à la molette autour du curseur.
        let resp = ui.interact(rect, ui.id().with("tree"), Sense::click_and_drag());
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let factor = (1.0 + scroll * 0.0015).clamp(0.5, 2.0);
                let new_zoom = (self.zoom * factor).clamp(0.01, 1.5);
                let factor = new_zoom / self.zoom;
                if let Some(cursor) = resp.hover_pos() {
                    let anchor = cursor - rect.center() - self.pan;
                    self.pan -= anchor * (factor - 1.0);
                }
                self.zoom = new_zoom;
            }
        }
        if resp.double_clicked() {
            self.fit(rect, &taken);
        }
        let main_attr = catalog.main_attribute(step);
        let overrides = &self.overrides;
        let attr_of = |n: &Node| overrides.get(&n.id).copied().unwrap_or(main_attr);
        let to_screen = |p: Pos2| rect.center() + self.pan + (p - self.center) * self.zoom;
        let is_fresh = |i: usize| prev.is_some() && taken.contains(&i) && !before.contains(&i) && self.nodes[i].kind != 4;

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(6, 6, 6, 235));
        // Médaillon central du jeu : son anneau passe par les six départs de classe.
        let start_ring = self
            .nodes
            .iter()
            .filter(|n| n.kind == 4)
            .map(|n| n.pos.to_vec2().length())
            .fold(0.0_f32, f32::max)
            .max(1000.0);
        painter.circle_filled(to_screen(Pos2::ZERO), start_ring * self.zoom, Color32::from_rgb(12, 10, 14));
        if let Some(tex) = icons.get(CENTER_CIRCLE).cloned() {
            let half = start_ring / RING_FRACTION * MEDALLION_SCALE;
            let r = Rect::from_center_size(to_screen(Pos2::ZERO), Vec2::splat(2.0 * half * self.zoom));
            painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let dim = Color32::from_rgb(52, 49, 44);
        // Certaines liaisons ne sont listées que d'un côté : on les trace toutes, une seule fois.
        let mut drawn: HashSet<(usize, usize)> = HashSet::new();
        for (i, n) in self.nodes.iter().enumerate() {
            for &(j, radius) in &n.links {
                // Les premiers nœuds sont sertis dans le médaillon : pas de trait vers le départ.
                if n.kind == 4 || self.nodes[j].kind == 4 || !drawn.insert((i.min(j), i.max(j))) {
                    continue;
                }
                let (a, b) = (to_screen(n.pos), to_screen(self.nodes[j].pos));
                // On écarte vite les liaisons hors champ (marge pour la courbure des arcs).
                if !rect.expand(radius.abs() * self.zoom).intersects(Rect::from_two_pos(a, b)) {
                    continue;
                }
                let both = taken.contains(&i) && taken.contains(&j);
                let color = if both && (is_fresh(i) || is_fresh(j)) { GREEN } else if both { GOLD } else { dim };
                let points: Vec<Pos2> = link_path(n, &self.nodes[j], radius).into_iter().map(to_screen).collect();
                painter.add(egui::Shape::line(points, Stroke::new(if both { 3.0_f32 } else { 1.5 }, color)));
            }
        }
        let mut hovered: Option<usize> = None;
        let cursor = resp.hover_pos();
        for (i, n) in self.nodes.iter().enumerate() {
            let p = to_screen(n.pos);
            if n.kind == 4 {
                // Départ de classe : ornement du médaillon ; croix dorée sur celui du build.
                if taken.contains(&i) {
                    if let Some(tex) = icons.get(ACTIVE_START).cloned() {
                        // Même échelle que le médaillon ; croix dans son trèfle, arcs vers l'extérieur.
                        let p = to_screen(n.pos * MEDALLION_SCALE);
                        let size = 2.0 * start_ring / RING_FRACTION * MEDALLION_SCALE * START_FRACTION * self.zoom;
                        let rect = Rect::from_min_size(p - START_ANCHOR * size, Vec2::splat(size));
                        let mut mesh = egui::Mesh::with_texture(tex.id());
                        mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        mesh.rotate(egui::emath::Rot2::from_angle(n.pos.x.atan2(-n.pos.y)), p);
                        painter.add(egui::Shape::mesh(mesh));
                    }
                }
                continue;
            }
            let r = (world_radius(n.kind) * self.zoom).clamp(1.5, 40.0);
            if !rect.expand(r).contains(p) {
                continue;
            }
            let on = taken.contains(&i);
            let mut color = if is_fresh(i) { GREEN } else if on { GOLD } else { Color32::from_rgb(90, 86, 78) };
            // Nœud d'attribut pris : couleur et icône de l'attribut choisi.
            let mut icon = n.icon.as_str();
            if n.attribute && on {
                let (_, c, ic) = ATTRIBUTES[attr_of(n)];
                icon = ic;
                if !is_fresh(i) {
                    color = c;
                }
            }
            // Icône du jeu dès que le nœud est assez grand à l'écran.
            let tex = if r >= 7.0 { icons.get(icon).cloned() } else { None };
            match tex {
                Some(tex) => {
                    painter.circle_filled(p, r, Color32::BLACK);
                    let tint = if on { Color32::WHITE } else { Color32::from_gray(95) };
                    painter.image(tex.id(), Rect::from_center_size(p, Vec2::splat(r * 1.6)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                    painter.circle_stroke(p, r, Stroke::new(if on { 2.5_f32 } else { 1.5 }, color));
                }
                None => {
                    painter.circle_filled(p, r, color);
                }
            }
            if let Some(&n) = set_of.get(&i) {
                let c = crate::ui::SET_COLORS[n as usize - 1];
                if n == set {
                    painter.circle_stroke(p, r + 3.0, Stroke::new(2.0_f32, c));
                } else {
                    painter.circle_stroke(p, r + 2.0, Stroke::new(1.5_f32, c.gamma_multiply(0.45)));
                }
            }
            if cursor.is_some_and(|c| c.distance(p) <= r + 3.0) {
                hovered = Some(i);
            }
        }
        let mut cycle: Option<String> = None;
        if let Some(i) = hovered {
            let n = &self.nodes[i];
            let r = (world_radius(n.kind) * self.zoom).clamp(1.5, 40.0);
            painter.circle_stroke(to_screen(n.pos), r + 4.0, Stroke::new(1.5_f32, Color32::WHITE));
            if n.attribute && taken.contains(&i) {
                if resp.secondary_clicked() {
                    cycle = Some(n.id.clone());
                }
                let (name, c, _) = ATTRIBUTES[attr_of(n)];
                let chosen = overrides.contains_key(&n.id);
                resp.on_hover_ui_at_pointer(|ui| {
                    ui.label(RichText::new(format!("+5 {name}")).color(c).strong());
                    let why = if chosen { "ton choix" } else { "choix supposé d'après les gemmes du build" };
                    ui.label(RichText::new(format!("{why} — clic droit pour changer")).color(Color32::GRAY).small());
                });
            } else if let Some(p) = catalog.passives.get(&n.id) {
                resp.on_hover_ui_at_pointer(|ui| {
                    ui.label(RichText::new(&p.name).color(GOLD).strong());
                    for s in &p.stats {
                        ui.label(RichText::new(s).color(Color32::from_rgb(136, 136, 255)));
                    }
                });
            }
        }
        if let Some(id) = cycle {
            let next = (self.overrides.get(&id).copied().unwrap_or(main_attr) + 1) % 3;
            self.overrides.insert(id, next);
            self.save_overrides();
        }
        // Légende sur fond, pour rester lisible au-dessus des nœuds.
        let nodes = if per_set == [0, 0] {
            format!("{counted} nœuds à cette étape")
        } else {
            let common = step.passives.len() - per_set[0] - per_set[1];
            format!("{common} communs + {} set I + {} set II (set {} affiché)", per_set[0], per_set[1], if set == 2 { "II" } else { "I" })
        };
        let legend = format!(
            "{nodes}{} · glisser : déplacer · molette : zoom · double-clic : recadrer",
            if prev.is_some() { format!(", dont {fresh_count} nouveaux en vert") } else { String::new() }
        );
        let galley = painter.layout_no_wrap(legend, egui::FontId::proportional(12.0), Color32::LIGHT_GRAY);
        let pos = rect.left_top() + Vec2::new(8.0, 8.0);
        painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(5.0), 3.0, Color32::from_rgba_unmultiplied(0, 0, 0, 200));
        painter.galley(pos, galley, Color32::LIGHT_GRAY);
    }
}

/// Case de largeur fixe, contenu aligné à gauche.
fn left_cell(ui: &mut egui::Ui, w: f32, h: f32, label: egui::Label) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(w);
            ui.set_max_width(w);
            ui.add(label)
        },
    )
    .inner
}

/// Onglet Caractéristiques : l'équipement porté face à ce que vise le build, mods additionnés.
/// Une colonne de noms, puis « Porté » et « Build ». Les valeurs portées sont comparées au build :
/// vert si elles l'atteignent, rouge sinon, gris sans équivalent dans le build. `penalty` :
/// pénalité de résistances élémentaires de la progression et ce qui l'explique.
pub fn stats(
    ui: &mut egui::Ui,
    worn: &crate::stats::Sheet,
    build: Option<&crate::stats::Sheet>,
    build_title: &str,
    penalty: &(f64, String),
) {
    use crate::stats::{Group, Line, RES_CAP, Total};
    let fmt = |v: f64| {
        if (v - v.round()).abs() < 0.05 {
            format!("{}", v.round())
        } else {
            format!("{v:.1}")
        }
    };
    let name_color = Color32::from_gray(200);
    let neutral = Color32::from_gray(225);
    let dim = Color32::from_gray(100);
    let (green, red) = (
        Color32::from_rgb(110, 200, 110),
        Color32::from_rgb(220, 95, 85),
    );
    // Porté face au build : vert s'il l'atteint, rouge sinon, gris si le build n'en vise pas.
    let versus = |mine: f64, target: Option<f64>| match target {
        Some(t) if t != 0.0 => {
            if mine >= t {
                green
            } else {
                red
            }
        }
        _ => {
            if mine == 0.0 {
                dim
            } else {
                neutral
            }
        }
    };
    let breakdown = |ui: &mut egui::Ui, t: &Total, suffix: &str| {
        for (slot, v) in &t.from {
            ui.label(format!("{slot} : {}{suffix}", fmt(*v)));
        }
    };
    // Largeurs : la moitié pour les noms, un quart pour chaque colonne de valeurs.
    let width = ui.available_width() - 16.0;
    let name_w = (width * 0.5).max(160.0);
    let col_w = ((width - name_w) / 2.0 - 8.0).max(90.0);
    let row = |ui: &mut egui::Ui, name: &str, cells: &mut dyn FnMut(&mut egui::Ui)| {
        ui.horizontal(|ui| {
            left_cell(
                ui,
                name_w,
                22.0,
                egui::Label::new(RichText::new(name).color(name_color).size(15.0)).truncate(),
            );
            cells(ui);
        });
    };
    let cell = |ui: &mut egui::Ui, text: String, color: Color32| {
        left_cell(
            ui,
            col_w,
            22.0,
            egui::Label::new(RichText::new(text).color(color).size(15.0).strong()),
        )
    };
    let total_row =
        |ui: &mut egui::Ui, name: &str, pick: &dyn Fn(&crate::stats::Sheet) -> &Total| {
            row(ui, name, &mut |ui| {
                let w = pick(worn);
                let t = build.map(pick);
                cell(ui, fmt(w.value), versus(w.value, t.map(|t| t.value)))
                    .on_hover_ui(|ui| breakdown(ui, w, ""));
                match t {
                    Some(t) => cell(ui, fmt(t.value), if t.value == 0.0 { dim } else { neutral })
                        .on_hover_ui(|ui| breakdown(ui, t, "")),
                    None => cell(ui, "—".into(), dim),
                };
            });
        };
    // Résistance effective, suivie en gris de la valeur de l'équipement quand elle diffère.
    let res_cell = |ui: &mut egui::Ui, eff: f64, raw: f64, color: Color32| {
        ui.allocate_ui_with_layout(
            egui::vec2(col_w, 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(col_w);
                ui.set_max_width(col_w);
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    RichText::new(format!("{} %", fmt(eff)))
                        .color(color)
                        .size(15.0)
                        .strong(),
                );
                if (raw - eff).abs() >= 0.05 {
                    ui.label(
                        RichText::new(format!("({} %)", fmt(raw)))
                            .color(Color32::from_gray(120))
                            .size(14.0),
                    );
                }
            },
        )
        .response
    };
    // Résistance effective : équipement + pénalité (élémentaires), plafonnée.
    let res_row = |ui: &mut egui::Ui, name: &str, idx: usize| {
        let pen = if idx < 3 { penalty.0 } else { 0.0 };
        let eff = |t: &Total| (t.value + pen).min(RES_CAP);
        let hover = |ui: &mut egui::Ui, t: &Total| {
            breakdown(ui, t, " %");
            ui.separator();
            ui.label(format!("Équipement : {} %", fmt(t.value)));
            if idx < 3 {
                ui.label(format!("Pénalité ({}) : {} %", penalty.1, fmt(pen)));
            }
            ui.label(format!("Plafond : {} %", fmt(RES_CAP)));
            let over = t.value + pen - RES_CAP;
            if over > 0.0 {
                ui.label(format!("{} % au-dessus du plafond", fmt(over)));
            }
        };
        row(ui, name, &mut |ui| {
            let w = &worn.resistances[idx];
            let t = build.map(|b| &b.resistances[idx]);
            // Comparé sur l'équipement brut : deux résistances au plafond peuvent différer.
            let color = match t {
                Some(t) if t.value != 0.0 => {
                    if w.value >= t.value {
                        green
                    } else {
                        red
                    }
                }
                _ => neutral,
            };
            res_cell(ui, eff(w), w.value, color).on_hover_ui(|ui| hover(ui, w));
            match t {
                Some(t) => res_cell(ui, eff(t), t.value, neutral).on_hover_ui(|ui| hover(ui, t)),
                None => cell(ui, "—".into(), dim),
            };
        });
    };
    // Titres de section sans trait : la ligne des colonnes, fixe, porte déjà le seul trait.
    let section = |ui: &mut egui::Ui, title: &str| {
        ui.add_space(10.0);
        ui.label(
            RichText::new(title)
                .color(crate::ui::GOLD)
                .size(13.0)
                .strong(),
        );
        ui.add_space(2.0);
    };

    if worn.pieces == 0 {
        ui.label(RichText::new("Aucune pièce suivie : Ctrl+Alt+C sur chaque pièce portée en jeu pour remplir la colonne « Porté ».").color(Color32::GRAY));
        ui.add_space(4.0);
    }
    // Titres des colonnes : fixes, le tableau défile dessous.
    ui.horizontal(|ui| {
        left_cell(ui, name_w, 20.0, egui::Label::new(""));
        left_cell(
            ui,
            col_w,
            20.0,
            egui::Label::new(RichText::new("Porté").color(crate::ui::GOLD).strong()),
        );
        left_cell(
            ui,
            col_w,
            20.0,
            egui::Label::new(RichText::new(build_title).color(crate::ui::GOLD).strong()).truncate(),
        );
    });
    ui.add_space(4.0);
    let line = ui.available_rect_before_wrap();
    ui.painter().hline(
        line.x_range(),
        line.top(),
        egui::Stroke::new(1.0_f32, crate::ui::GOLD_DIM.gamma_multiply(0.6)),
    );
    ui.add_space(2.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            section(ui, "ATTRIBUTS");
            total_row(ui, "Force", &|s| &s.attributes[0]);
            total_row(ui, "Dextérité", &|s| &s.attributes[1]);
            total_row(ui, "Intelligence", &|s| &s.attributes[2]);
            section(ui, "VIE ET DÉFENSES");
            total_row(ui, "Vie", &|s| &s.life);
            total_row(ui, "Mana", &|s| &s.mana);
            total_row(ui, "Esprit", &|s| &s.spirit);
            total_row(ui, "Armure", &|s| &s.defences[0]);
            total_row(ui, "Évasion", &|s| &s.defences[1]);
            total_row(ui, "Bouclier d'énergie", &|s| &s.defences[2]);
            section(ui, "RÉSISTANCES");
            res_row(ui, "Feu", 0);
            res_row(ui, "Froid", 1);
            res_row(ui, "Foudre", 2);
            res_row(ui, "Chaos", 3);

            // Autres mods : le texte du mod en nom, ses nombres dans chaque colonne.
            let empty: Vec<Line> = Vec::new();
            for group in [
                Group::Damage,
                Group::Gems,
                Group::Resources,
                Group::Defence,
                Group::Other,
            ] {
                let mine = worn.lines.get(&group).unwrap_or(&empty);
                let theirs = build.and_then(|b| b.lines.get(&group)).unwrap_or(&empty);
                let mut templates: Vec<&str> = mine
                    .iter()
                    .chain(theirs.iter())
                    .map(|l| l.template.as_str())
                    .collect();
                templates.sort();
                templates.dedup();
                if templates.is_empty() {
                    continue;
                }
                section(ui, &group.label().to_uppercase());
                for t in templates {
                    let a = mine.iter().find(|l| l.template == t);
                    let b = theirs.iter().find(|l| l.template == t);
                    row(ui, t, &mut |ui| {
                        let color = versus(a.map(|l| l.sum()).unwrap_or(0.0), b.map(|l| l.sum()));
                        let r = cell(
                            ui,
                            a.map(|l| l.value_text()).unwrap_or_else(|| "—".into()),
                            if a.is_none() && b.is_some() {
                                red
                            } else {
                                color
                            },
                        );
                        if let Some(l) = a {
                            r.on_hover_text(l.from.join(", "));
                        }
                        let r = cell(
                            ui,
                            b.map(|l| l.value_text()).unwrap_or_else(|| "—".into()),
                            if b.is_some() { neutral } else { dim },
                        );
                        if let Some(l) = b {
                            r.on_hover_text(l.from.join(", "));
                        }
                    });
                }
            }
            if !worn.weapon_local.is_empty() {
                section(ui, "PROPRE À L'ARME PORTÉE");
                for (slot, lines) in &worn.weapon_local {
                    ui.label(RichText::new(slot).color(crate::ui::GOLD_DIM));
                    for l in lines {
                        ui.label(RichText::new(l).color(Color32::GRAY).size(14.0));
                    }
                }
            }
        });
}
