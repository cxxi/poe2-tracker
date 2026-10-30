//! Arbre de l'Atlas, lu dans les fichiers du jeu du joueur : le graphe (positions, liaisons),
//! les passifs (nom, icône, notable) et le texte de leurs effets, en français. Gardé en cache
//! disque tant que le jeu ne change pas ces fichiers.

use std::collections::HashSet;
use std::path::Path;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::bundles;
use crate::dat::Dat;
use crate::statdesc::Descriptions;
use crate::ui::GOLD;

const GRAPH: &str = "metadata/atlasskillgraphs/atlasskillgraph.psg";
const PASSIVES_FR: &str = "data/balance/french/passiveskills.datc64";
const PASSIVES: &str = "data/balance/passiveskills.datc64";
const STATS: &str = "data/balance/stats.datc64";
const DESCRIPTIONS: &str = "data/statdescriptions/atlas_stat_descriptions.csd";
/// Inclus par celui de l'Atlas : effets communs aux maps.
const MAP_DESCRIPTIONS: &str = "data/statdescriptions/map_stat_descriptions.csd";
const CACHE: &str = "atlas_tree_v1.json";

/// Rayons des orbites (mêmes que l'arbre des talents, constantes de Path of Building).
const RADII: [f32; 10] = [0.0, 82.0, 162.0, 335.0, 493.0, 662.0, 846.0, 251.0, 1080.0, 1322.0];

// Colonnes de passiveskills.datc64 (vérifiées : la colonne du graphe doit retrouver les nœuds).
const COL_ID: usize = 0;
const COL_ICON: usize = 8;
const COL_STATS: usize = 16;
const COL_VALUES: usize = 32;
const COL_GRAPH: usize = 48;
const COL_NAME: usize = 50;
const COL_KEYSTONE: usize = 74;
const COL_NOTABLE: usize = 75;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AtlasNode {
    /// Identifiant du passif, celui du journal du jeu (« AtlasRitualNotable3_ »).
    pub id: String,
    pub name: String,
    pub icon: String,
    /// 0 petit passif, 1 notable, 2 keystone, 5 départ d'un arbre.
    pub kind: u8,
    pub x: f32,
    pub y: f32,
    pub gx: f32,
    pub gy: f32,
    pub orbit: u8,
    /// Nœuds reliés et rayon d'arc signé de la liaison (0 : droite).
    pub links: Vec<(usize, f32)>,
    pub stats: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct AtlasTree {
    pub nodes: Vec<AtlasNode>,
    /// Emplacement des fichiers sources dans les archives : le cache vaut tant qu'il ne change pas.
    key: String,
}

struct GraphNode {
    graph_id: u32,
    orbit: u8,
    index: u32,
    edges: Vec<(u32, i32)>,
}

/// Graphe .psg : version, type, orbites, racines, puis les groupes (centre, nœuds, liaisons).
fn parse_graph(d: &[u8]) -> Option<Vec<((f32, f32), Vec<GraphNode>)>> {
    let mut p = 2;
    let take = |p: &mut usize, n: usize| -> Option<&[u8]> {
        let s = d.get(*p..*p + n)?;
        *p += n;
        Some(s)
    };
    let u32_at = |p: &mut usize| -> Option<u32> { Some(u32::from_le_bytes(take(p, 4)?.try_into().ok()?)) };
    let orbits = *d.get(p)? as usize;
    p += 1 + orbits;
    let roots = u32_at(&mut p)? as usize;
    p += roots * 8;
    let groups = u32_at(&mut p)? as usize;
    let mut out = Vec::with_capacity(groups);
    for _ in 0..groups {
        let x = f32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?);
        let y = f32::from_le_bytes(take(&mut p, 4)?.try_into().ok()?);
        p += 9; // association, fond, emplacement de joyau
        let count = u32_at(&mut p)? as usize;
        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            let graph_id = u32_at(&mut p)?;
            let orbit = u32_at(&mut p)? as u8;
            let index = u32_at(&mut p)?;
            let edges = u32_at(&mut p)? as usize;
            let mut list = Vec::with_capacity(edges);
            for _ in 0..edges {
                let to = u32_at(&mut p)?;
                let curve = u32_at(&mut p)? as i32;
                list.push((to, curve));
            }
            nodes.push(GraphNode { graph_id, orbit, index, edges: list });
        }
        out.push(((x, y), nodes));
    }
    Some(out)
}

/// Nombre d'emplacements sur une orbite (angles réguliers).
const SLOTS: [u32; 10] = [1, 12, 24, 24, 72, 72, 72, 24, 72, 144];

fn key_of(index: &bundles::Index) -> String {
    [GRAPH, PASSIVES_FR, PASSIVES, STATS, DESCRIPTIONS, MAP_DESCRIPTIONS]
        .iter()
        .map(|f| index.entry(f).map(|e| format!("{}:{}:{}", e.bundle, e.offset, e.size)).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",")
}

fn build(index: &bundles::Index) -> Result<AtlasTree, String> {
    let read = |f: &str| index.read(f).map_err(|e| e.to_string());
    let graph = parse_graph(&read(GRAPH)?).ok_or("graphe de l'Atlas illisible")?;
    let passives = Dat::new(read(PASSIVES_FR).or_else(|_| read(PASSIVES))?).ok_or("table des passifs illisible")?;
    let stats = Dat::new(read(STATS)?).ok_or("table des stats illisible")?;
    let utf16 = |raw: Vec<u8>| {
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_start_matches('\u{feff}').to_string()
    };
    // Celles de l'Atlas d'abord, puis celles des maps qu'il inclut.
    let text = utf16(read(DESCRIPTIONS)?) + "\n" + &read(MAP_DESCRIPTIONS).map(utf16).unwrap_or_default();
    let descriptions = Descriptions::parse(&text);
    // Lignes des passifs de l'Atlas par identifiant de graphe.
    let rows: std::collections::HashMap<u16, usize> =
        (0..passives.rows).filter(|r| passives.string(*r, COL_ID).starts_with("Atlas")).map(|r| (passives.u16(r, COL_GRAPH), r)).collect();
    let total: usize = graph.iter().map(|(_, n)| n.len()).sum();
    let found = graph.iter().flat_map(|(_, n)| n).filter(|n| rows.contains_key(&(n.graph_id as u16))).count();
    if found * 10 < total * 9 {
        return Err(format!("passifs de l'Atlas introuvables ({found} sur {total}) : le format des fichiers du jeu a changé"));
    }
    let mut nodes = Vec::with_capacity(total);
    let mut by_graph = std::collections::HashMap::new();
    for ((gx, gy), group) in &graph {
        for n in group {
            let Some(&r) = rows.get(&(n.graph_id as u16)) else { continue };
            let slots = SLOTS.get(n.orbit as usize).copied().unwrap_or(1).max(1);
            let angle = std::f32::consts::TAU * n.index as f32 / slots as f32;
            let radius = RADII.get(n.orbit as usize).copied().unwrap_or(0.0);
            let id = passives.string(r, COL_ID);
            let kind = if id.contains("Start") { 5 } else if passives.u8(r, COL_KEYSTONE) == 1 { 2 } else if passives.u8(r, COL_NOTABLE) == 1 { 1 } else { 0 };
            let values: Vec<(String, i64)> = passives
                .row_list(r, COL_STATS)
                .iter()
                .enumerate()
                .map(|(k, s)| (stats.string(*s, 0), if k < 4 { passives.i32(r, COL_VALUES + 4 * k) as i64 } else { 0 }))
                .collect();
            by_graph.insert(n.graph_id, nodes.len());
            nodes.push(AtlasNode {
                id,
                name: passives.string(r, COL_NAME),
                icon: passives.string(r, COL_ICON),
                kind,
                x: gx + radius * angle.sin(),
                y: gy - radius * angle.cos(),
                gx: *gx,
                gy: *gy,
                orbit: n.orbit,
                links: vec![],
                stats: descriptions.describe(&values, "French"),
            });
        }
    }
    // Liaisons, une fois les numéros des nœuds connus.
    let mut i = 0;
    for (_, group) in &graph {
        for n in group {
            if !rows.contains_key(&(n.graph_id as u16)) {
                continue;
            }
            nodes[i].links = n
                .edges
                .iter()
                .filter_map(|(to, curve)| {
                    let j = *by_graph.get(to)?;
                    let r = RADII.get(curve.unsigned_abs() as usize).copied().filter(|_| *curve != 0).map_or(0.0, |r| r.copysign(*curve as f32));
                    Some((j, r))
                })
                .collect();
            i += 1;
        }
    }
    Ok(AtlasTree { nodes, key: key_of(index) })
}

/// Arbre depuis le cache, ou relu dans les fichiers du jeu s'ils ont changé.
pub fn load(cache_dir: &Path, game_dir: &Path) -> Result<AtlasTree, String> {
    let index = bundles::Index::open(game_dir).map_err(|e| format!("fichiers du jeu illisibles : {e}"))?;
    let key = key_of(&index);
    let file = cache_dir.join(CACHE);
    if let Some(tree) = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<AtlasTree>(&s).ok()).filter(|t| t.key == key) {
        return Ok(tree);
    }
    let tree = build(&index)?;
    let _ = std::fs::write(&file, serde_json::to_string(&tree).unwrap_or_default());
    Ok(tree)
}

/// Rayon d'un nœud en unités de l'arbre, selon son type.
fn radius(kind: u8) -> f32 {
    match kind {
        2 => 95.0,
        1 => 70.0,
        5 => 60.0,
        _ => 45.0,
    }
}

/// Vue de l'arbre : déplacement à la souris, zoom à la molette, comme l'arbre des talents.
#[derive(Default)]
pub struct AtlasView {
    center: Pos2,
    zoom: f32,
    pan: Vec2,
    fitted: bool,
    /// Taille de la vue au dernier cadrage : on recadre quand elle change (plein écran…).
    fitted_size: Vec2,
}

impl AtlasView {
    /// Recadre au prochain affichage.
    pub fn recenter(&mut self) {
        self.fitted = false;
    }

    /// Cadre sur les nœuds posés (ou tout l'arbre).
    fn fit(&mut self, rect: Rect, tree: &AtlasTree, taken: &HashSet<usize>) {
        let pts: Vec<Pos2> = tree.nodes.iter().enumerate().filter(|(i, _)| taken.is_empty() || taken.contains(i)).map(|(_, n)| Pos2::new(n.x, n.y)).collect();
        if pts.is_empty() {
            return;
        }
        let bb = Rect::from_points(&pts).expand(500.0);
        let size = bb.size().max(Vec2::splat(4000.0));
        self.center = bb.center();
        self.zoom = (rect.width() / size.x).min(rect.height() / size.y) * 0.95;
        self.pan = Vec2::ZERO;
        self.fitted = true;
    }

    /// `allocated` : identifiants des passifs posés (journal du jeu).
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, tree: &AtlasTree, allocated: &HashSet<&str>, icons: &mut crate::passive_icons::PassiveIcons) {
        let mut taken: HashSet<usize> = tree.nodes.iter().enumerate().filter(|(_, n)| allocated.contains(n.id.as_str())).map(|(i, _)| i).collect();
        // Le départ d'un arbre compte comme pris dès qu'un de ses voisins l'est.
        let starts: Vec<usize> = tree.nodes.iter().enumerate().filter(|(_, n)| n.kind == 5 && n.links.iter().any(|(j, _)| taken.contains(j))).map(|(i, _)| i).collect();
        taken.extend(starts);
        if !self.fitted || (self.fitted_size - rect.size()).length() > 1.0 {
            self.fitted_size = rect.size();
            self.fit(rect, tree, &HashSet::new());
        }
        let resp = ui.interact(rect, ui.id().with("atlas_tree"), Sense::click_and_drag());
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
            self.fit(rect, tree, &HashSet::new());
        }
        let to_screen = |p: Pos2| rect.center() + self.pan + (p - self.center) * self.zoom;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(6, 6, 6, 235));
        let dim = Color32::from_rgb(52, 49, 44);
        let mut drawn: HashSet<(usize, usize)> = HashSet::new();
        for (i, n) in tree.nodes.iter().enumerate() {
            for &(j, r) in &n.links {
                if !drawn.insert((i.min(j), i.max(j))) {
                    continue;
                }
                let m = &tree.nodes[j];
                let (a, b) = (to_screen(Pos2::new(n.x, n.y)), to_screen(Pos2::new(m.x, m.y)));
                if !rect.expand(r.abs() * self.zoom).intersects(Rect::from_two_pos(a, b)) {
                    continue;
                }
                let both = taken.contains(&i) && taken.contains(&j);
                let points: Vec<Pos2> = crate::views::link_points((Pos2::new(n.x, n.y), Pos2::new(n.gx, n.gy), n.orbit), (Pos2::new(m.x, m.y), Pos2::new(m.gx, m.gy), m.orbit), r)
                    .into_iter()
                    .map(to_screen)
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(if both { 3.0_f32 } else { 1.5 }, if both { GOLD } else { dim })));
            }
        }
        let cursor = resp.hover_pos();
        let mut hovered = None;
        for (i, n) in tree.nodes.iter().enumerate() {
            let p = to_screen(Pos2::new(n.x, n.y));
            let r = (radius(n.kind) * self.zoom).clamp(1.5, 40.0);
            if !rect.expand(r).contains(p) {
                continue;
            }
            let on = taken.contains(&i);
            let color = if on { GOLD } else { Color32::from_rgb(90, 86, 78) };
            match if r >= 7.0 { icons.get(&n.icon).cloned() } else { None } {
                Some(tex) => {
                    painter.circle_filled(p, r, Color32::BLACK);
                    let tint = if on { Color32::WHITE } else { Color32::from_gray(95) };
                    // Dans le cercle du nœud, sous son contour.
                    crate::ui::circle_image(&painter, tex.id(), p, r * 0.92, tint);
                    painter.circle_stroke(p, r, Stroke::new(if on { 2.5_f32 } else { 1.5 }, color));
                }
                None => {
                    painter.circle_filled(p, r, color);
                }
            }
            if cursor.is_some_and(|c| c.distance(p) <= r + 3.0) {
                hovered = Some(i);
            }
        }
        if let Some(i) = hovered {
            let n = &tree.nodes[i];
            let r = (radius(n.kind) * self.zoom).clamp(1.5, 40.0);
            painter.circle_stroke(to_screen(Pos2::new(n.x, n.y)), r + 4.0, Stroke::new(1.5_f32, Color32::WHITE));
            let on = taken.contains(&i);
            let name = if n.name.is_empty() { "Départ" } else { n.name.as_str() };
            let note = (n.kind != 5).then_some(if on { "Posé" } else { "Pas posé" });
            resp.on_hover_ui_at_pointer(|ui| crate::ui::node_tip(ui, None, name, GOLD, None, &n.stats, note));
        }
        let legend = format!("{} passifs posés · glisser : déplacer · molette : zoom · double-clic : recadrer", allocated.len());
        let galley = painter.layout_no_wrap(legend, egui::FontId::proportional(12.0), Color32::LIGHT_GRAY);
        let pos = rect.left_top() + Vec2::new(8.0, 8.0);
        painter.rect_filled(Rect::from_min_size(pos, galley.size()).expand(5.0), 3.0, Color32::from_rgba_unmultiplied(0, 0, 0, 200));
        painter.galley(pos, galley, Color32::LIGHT_GRAY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arbre lu dans le vrai jeu : POE2_GAME=… cargo test real_atlas_tree -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_atlas_tree() {
        let Ok(game) = std::env::var("POE2_GAME") else { return };
        let index = bundles::Index::open(Path::new(&game)).unwrap();
        let tree = build(&index).unwrap();
        eprintln!("{} nœuds", tree.nodes.len());
        for id in ["AtlasRitualNotable3_", "AtlasRitualSmall1", "AtlasAbyssNotable3", "AtlasGenericStart"] {
            let n = tree.nodes.iter().find(|n| n.id == id).unwrap();
            eprintln!("{id} : {} (type {}) {:?} — {} liaisons", n.name, n.kind, n.stats, n.links.len());
        }
        let raw: Vec<&String> = tree.nodes.iter().flat_map(|n| &n.stats).filter(|s| !s.contains(' ')).collect();
        eprintln!("{} effets sans description : {:?}", raw.len(), &raw[..raw.len().min(10)]);
    }
}
