//! Dessin de la grille d'équipement (disposition de l'écran de personnage de PoE2)
//! et des fiches d'objets façon infobulle du jeu.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, Vec2,
};

use crate::gear::Slot;
use crate::item::{Item, Rarity};

/// Zone de chaque emplacement (x, y, largeur, hauteur) en pixels de l'image de fond du jeu
/// (inventorypanelupperbackground, 1320 × 1028), relevée sur ses cadres.
pub fn layout(slot: Slot) -> [f32; 4] {
    match slot {
        Slot::Weapon | Slot::Weapon2 => [132.0, 98.0, 220.0, 433.0],
        Slot::Helmet => [543.0, 73.0, 224.0, 220.0],
        Slot::Offhand | Slot::Offhand2 => [956.0, 98.0, 225.0, 433.0],
        Slot::Body => [543.0, 308.0, 224.0, 327.0],
        Slot::Amulet => [802.0, 283.0, 118.0, 117.0],
        Slot::RingLeft => [389.0, 415.0, 117.0, 117.0],
        Slot::RingRight => [802.0, 415.0, 118.0, 117.0],
        Slot::Gloves => [283.0, 546.0, 223.0, 224.0],
        Slot::Boots => [802.0, 546.0, 225.0, 224.0],
        Slot::Belt => [543.0, 650.0, 224.0, 120.0],
        // Fioles : l'intérieur exact de leur cadre (mesuré sur les images du jeu, cadre de la vie
        // orné à gauche, de la mana à droite).
        Slot::LifeFlask => [340.0, 774.0, 100.0, 202.0],
        Slot::Charm1 => [491.0, 825.0, 104.0, 104.0],
        Slot::Charm2 => [597.0, 825.0, 104.0, 104.0],
        Slot::Charm3 => [703.0, 825.0, 104.0, 104.0],
        Slot::ManaFlask => [865.0, 774.0, 100.0, 202.0],
    }
}
/// Sélecteur du set d'armes I / II, centré sous les charmes (repère du fond).
pub const SET_SWITCH: [f32; 3] = [654.0, 976.0, 34.0];

/// Couleurs des sets d'armes I et II (onglets, passifs propres à un set).
pub const SET_COLORS: [Color32; 2] = [Color32::from_rgb(225, 105, 80), Color32::from_rgb(90, 165, 235)];

/// Onglets I / II ; `marked[i]` : un unique du build manque dans ce set. Vrai si changé.
/// Libellés, police et largeur d'une moitié de l'interrupteur de set.
fn switch_parts(ui: &egui::Ui, height: f32, marked: [bool; 2]) -> ([String; 2], FontId, f32) {
    let font = FontId::proportional((height * 0.45).clamp(11.0, 15.0));
    let labels = [0, 1].map(|i| format!("Set {}{}", ["I", "II"][i], if marked[i] { " ★" } else { "" }));
    let text_w = labels.iter().map(|l| ui.fonts(|f| f.layout_no_wrap(l.clone(), font.clone(), GOLD).size().x)).fold(0.0, f32::max);
    // Calé sur les pixels : bordure et séparateur se rejoignent sans dépasser.
    (labels, font, (text_w + 2.0 * (height * 0.3).clamp(6.0, 10.0)).round())
}

/// Taille de l'interrupteur de set, pour le placer dans une mise en page.
pub fn set_tabs_size(ui: &egui::Ui, height: f32, marked: [bool; 2]) -> Vec2 {
    Vec2::new(2.0 * switch_parts(ui, height, marked).2, height.round())
}

/// Interrupteur Set I / Set II : un seul cadre, la moitié du set affiché allumée, largeur
/// ajustée au texte. `marked[i]` : un unique du build manque dans ce set. `colored` : chaque
/// moitié prend la couleur de son set (arbre des talents). Vrai si le set a changé.
pub fn set_tabs(ui: &mut egui::Ui, center: Pos2, height: f32, id: &str, set: &mut u8, marked: [bool; 2], colored: bool) -> bool {
    let (labels, font, w) = switch_parts(ui, height, marked);
    let rect = Rect::from_center_size(center, Vec2::new(2.0 * w, height.round()));
    let rect = Rect::from_min_size(rect.min.round(), rect.size());
    let radius = 4u8;
    let hovered_any = ui.rect_contains_pointer(rect);
    let line = Stroke::new(1.0_f32, if hovered_any { GOLD } else { GOLD_DIM });
    ui.painter().rect_filled(rect, CornerRadius::same(radius), Color32::from_black_alpha(190));
    let mut changed = false;
    for (i, label) in labels.into_iter().enumerate() {
        let n = i as u8 + 1;
        let r = Rect::from_min_size(rect.min + Vec2::new(i as f32 * w, 0.0), Vec2::new(w, height));
        let resp = ui.interact(r, ui.id().with((id, n)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = *set == n;
        let painter = ui.painter();
        if on {
            // Coins arrondis seulement côté extérieur.
            let corners = if i == 0 { CornerRadius { nw: radius, sw: radius, ne: 0, se: 0 } } else { CornerRadius { nw: 0, sw: 0, ne: radius, se: radius } };
            painter.rect_filled(r, corners, if colored { SET_COLORS[i].gamma_multiply(0.4) } else { Color32::from_rgb(58, 48, 30) });
        } else if resp.hovered() {
            painter.rect_filled(r, CornerRadius::ZERO, Color32::from_white_alpha(8));
        }
        let color = match (colored, on) {
            (true, true) => Color32::WHITE,
            (true, false) => SET_COLORS[i],
            (false, true) => GOLD,
            (false, false) => Color32::GRAY,
        };
        painter.text(r.center(), Align2::CENTER_CENTER, &label, font.clone(), color);
        let resp = resp.on_hover_text(match (marked[i], colored) {
            (true, _) => "Un unique du build manque dans ce set",
            (false, true) => "Passifs de ce set entourés de sa couleur",
            (false, false) => "Set d'armes affiché",
        });
        if resp.clicked() && !on {
            *set = n;
            changed = true;
        }
    }
    ui.painter().rect_stroke(rect, CornerRadius::same(radius), line, StrokeKind::Inside);
    // Entre les deux traits horizontaux de la bordure (dessinée à l'intérieur du cadre).
    ui.painter().vline(rect.min.x + w, egui::Rangef::new(rect.top() + 1.0, rect.bottom() - 1.0), line);
    changed
}

/// Épaisseur de la bordure dessinée par le fond du jeu autour d'une case (repère du fond) :
/// la case se dessine à l'intérieur, sans la masquer.
pub fn slot_inset(slot: Slot) -> f32 {
    match slot.in_set(1) {
        Slot::Amulet | Slot::RingLeft | Slot::RingRight => 9.0,
        Slot::LifeFlask | Slot::ManaFlask => 0.0,
        Slot::Charm1 | Slot::Charm2 | Slot::Charm3 => 5.0,
        _ => 11.0,
    }
}

/// Panneau de pierre du fond (les cases d'équipement dessinées, les statues), à l'intérieur de
/// la bordure : recouvert hors de l'onglet Équipement.
pub const STONE_PANEL: [f32; 4] = [30.0, 52.0, 1248.0, 966.0];
pub const STONE_PANEL_FILL: Color32 = Color32::from_rgb(14, 13, 12);

/// Bandeau vide en haut de l'image de fond, entre ses deux ornements d'angle :
/// emplacement de la barre personnage / build.
/// Mesuré sur l'image : zone noire de 40 px de haut, ornements d'angle sur ~20 px.
/// Centré sur le cadre doré du panneau, juste dessous (x 4 → 1305 sur l'image, qui n'est pas
/// symétrique) : même écart de part et d'autre.
pub const HEADER_STRIP: [f32; 4] = [28.5, 0.0, 1252.0, 40.0];
/// Marge verticale entre la barre et les bords du bandeau.
const BAR_MARGIN: f32 = 4.0;

/// Décalage vers le bas de la grille quand le bandeau est trop bas pour la barre
/// (`bar_h` : hauteur réelle de la barre, mesurée à l'affichage précédent).
pub fn top_pad(scale: f32, bar_h: f32) -> f32 {
    (bar_h + 2.0 * BAR_MARGIN - HEADER_STRIP[3] * scale).max(0.0)
}

/// Plus grande échelle de la grille (bandeau et décalage compris) dans w × h.
pub fn canvas_scale(w: f32, h: f32, bar_h: f32) -> f32 {
    let need = bar_h + 2.0 * BAR_MARGIN;
    let by_width = w / CANVAS_W;
    let full = h / CANVAS_H;
    let by_height = if HEADER_STRIP[3] * full >= need { full } else { (h - need) / (CANVAS_H - HEADER_STRIP[3]) };
    by_width.min(by_height).max(0.05)
}
/// Zone de contenu des onglets autres que l'équipement (intérieur du panneau de pierre).
pub const VIEW_AREA: [f32; 4] = [52.0, 62.0, 1216.0, 940.0];
pub const CANVAS_W: f32 = 1320.0;
pub const CANVAS_H: f32 = 1028.0;
/// Cadres posés sur le fond (fioles, charmes), même repère, à leur taille d'origine.
/// Positions relevées en jeu ; les images ont des marges transparentes (2 px à gauche
/// pour la vie, 20 px pour la mana).
pub const LIFE_FRAME: [f32; 4] = [309.0, 763.0, 172.0, 232.0];
pub const MANA_FRAME: [f32; 4] = [834.0, 763.0, 172.0, 232.0];
pub const CHARM_FRAME: [f32; 4] = [484.0, 818.0, 340.0, 128.0];

/// Rectangle à l'écran pour une zone exprimée en pixels de l'image de fond.
pub fn place(area: Rect, scale: f32, [x, y, w, h]: [f32; 4]) -> Rect {
    Rect::from_min_size(area.min + Vec2::new(x * scale, y * scale), Vec2::new(w * scale, h * scale))
}

pub fn draw_image(painter: &egui::Painter, rect: Rect, tex: &TextureHandle) {
    painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

pub fn rarity_color(r: Rarity) -> Color32 {
    match r {
        Rarity::Normal => Color32::from_rgb(200, 200, 200),
        Rarity::Magic => Color32::from_rgb(136, 136, 255),
        Rarity::Rare => Color32::from_rgb(255, 255, 119),
        Rarity::Unique => Color32::from_rgb(175, 96, 37),
        Rarity::Currency => Color32::from_rgb(170, 158, 130),
        Rarity::Other => Color32::from_rgb(180, 180, 180),
    }
}

const MOD_COLOR: Color32 = Color32::from_rgb(136, 136, 255);
const DIM: Color32 = Color32::from_rgb(127, 127, 127);
const SLOT_FILL: Color32 = Color32::from_rgb(22, 20, 18);
const SLOT_EMPTY_STROKE: Color32 = Color32::from_rgb(70, 62, 50);

pub enum SlotAction {
    None,
    Clear,
}

/// Cases qui ne peuvent pas recevoir de pièce.
pub enum Blocked<'a> {
    /// Main gauche prise par une arme à deux mains (on la montre en transparence).
    ByTwoHander(Option<&'a TextureHandle>),
    /// Emplacement de charme que la ceinture n'ouvre pas (icône de verrou du jeu si disponible).
    ClosedCharm(Option<&'a TextureHandle>),
}

/// `framed` : dessiner nos propres cadres (quand le fond du jeu n'est pas disponible).
pub fn blocked_slot(ui: &mut egui::Ui, rect: Rect, slot: Slot, why: Blocked, framed: bool, star: bool) {
    let resp = ui.interact(rect, ui.id().with(("blocked", slot)), Sense::hover());
    let hovered = resp.hovered() && !over_star(ui, rect, star);
    let painter = ui.painter();
    if framed {
        painter.rect_filled(rect, CornerRadius::same(3), SLOT_FILL.gamma_multiply(0.6));
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0_f32, SLOT_EMPTY_STROKE.gamma_multiply(0.5)), StrokeKind::Inside);
    }
    match why {
        Blocked::ByTwoHander(tex) => {
            painter.rect_filled(rect, CornerRadius::same(3), SLOT_FILL);
            if let Some(tex) = tex {
                paint_texture(painter, rect, tex, Color32::from_white_alpha(45));
            }
            if hovered {
                resp.on_hover_text("Occupée par ton arme à deux mains");
            }
        }
        Blocked::ClosedCharm(blocker) => {
            if let Some(tex) = blocker {
                paint_texture(painter, rect, tex, Color32::WHITE);
            }
            if hovered {
                resp.on_hover_text("Ta ceinture n'ouvre pas cet emplacement de charme");
            }
        }
    }
}

fn paint_texture(painter: &egui::Painter, rect: Rect, tex: &TextureHandle, tint: Color32) {
    let inner = rect.shrink(3.0);
    let size = tex.size_vec2();
    let scale = (inner.width() / size.x).min(inner.height() / size.y);
    let img = Rect::from_center_size(inner.center(), size * scale);
    painter.image(tex.id(), img, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
}

/// Une case de la grille. `glow` colore le cadre (case comparée, phase 2). `star` : la case
/// porte l'étoile d'un unique du build, qui garde son infobulle à elle.
pub fn slot(
    ui: &mut egui::Ui,
    rect: Rect,
    slot: Slot,
    item: Option<&Item>,
    texture: Option<&TextureHandle>,
    glow: Option<Color32>,
    framed: bool,
    star: bool,
    sockets: Option<&SocketView>,
    build: Option<&BuildCard>,
    ghost: Option<&TextureHandle>,
) -> SlotAction {
    let id = ui.id().with(("slot", slot));
    let resp = ui.interact(rect, id, Sense::click());
    let hovered = resp.hovered() && !over_star(ui, rect, star);
    let painter = ui.painter();
    // Sur le fond du jeu, on masque la silhouette de l'emplacement sous un objet suivi, et sous
    // la pièce recommandée d'une case vide (elle se confondrait avec la silhouette).
    if framed || item.is_some() || ghost.is_some() {
        painter.rect_filled(rect, CornerRadius::same(3), SLOT_FILL);
    }
    // Sur le fond du jeu, seuls les objets suivis reçoivent un liseré de rareté.
    if framed || item.is_some() || glow.is_some() {
        let stroke_color = glow.unwrap_or_else(|| item.map(|i| rarity_color(i.rarity)).unwrap_or(SLOT_EMPTY_STROKE));
        let width: f32 = if glow.is_some() { 3.0 } else if item.is_some() { 1.5 } else { 1.0 };
        let alpha = if framed || glow.is_some() { 1.0 } else { 0.6 };
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(width, stroke_color.gamma_multiply(alpha)), StrokeKind::Inside);
    }
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), Color32::from_white_alpha(10));
    }

    let small = rect.height() < 40.0;
    match (item, texture) {
        (Some(_), Some(tex)) => {
            paint_texture(painter, rect, tex, Color32::WHITE);
            if let Some(v) = sockets {
                paint_sockets(painter, rect, v);
            }
        }
        (Some(it), None) => {
            painter.text(rect.center(), Align2::CENTER_CENTER, &it.name, FontId::proportional(if small { 8.0 } else { 11.0 }), rarity_color(it.rarity));
        }
        (None, _) => {
            // Case vide : la pièce que le build recommande, en noir et blanc.
            match ghost {
                Some(tex) => paint_texture(painter, rect, tex, Color32::from_gray(150)),
                None if framed && !small => {
                    painter.text(rect.center(), Align2::CENTER_CENTER, slot.label(), FontId::proportional(11.0), DIM);
                }
                None => {}
            }
            if hovered {
                match build {
                    // Rien à comparer : seulement ce que le build recommande ici.
                    Some(b) => {
                        resp.clone().on_hover_ui(|ui| {
                            ui.label(RichText::new(format!("{} : non suivi — Ctrl+Alt+C sur ta pièce en jeu", slot.label())).color(Color32::GRAY).small());
                            ui.separator();
                            build_card(ui, b);
                        });
                    }
                    None => {
                        resp.clone().on_hover_text(format!("{} : non suivi — Ctrl+Alt+C sur ta pièce en jeu", slot.label()));
                    }
                }
            }
        }
    }

    // Le temps que la fiche s'ouvre, un petit indicateur dans le coin montre que ça vient ;
    // une fois la fiche ouverte, il laisse place à une croix pour ne plus suivre la pièce.
    // Le pointeur est suivi sur toute la case (croix comprise), pas seulement sur sa zone
    // de clic, sinon passer sur la croix relancerait l'indicateur.
    let inside = ui.rect_contains_pointer(rect) && !over_star(ui, rect, star);
    let since_id = id.with("hover_since");
    // Même taille et même marge que l'étoile d'unique, dans le coin opposé.
    let hit = clear_rect(rect);
    let (corner, size) = (hit.center(), hit.width() / 1.1);
    let mut clear = false;
    if item.is_some() && inside {
        let now = ui.input(|i| i.time);
        let since = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(since_id, || now));
        let delay = ui.style().interaction.tooltip_delay as f64;
        if now - since < delay {
            egui::Spinner::new().size(size).color(GOLD).paint_at(ui, Rect::from_center_size(corner, egui::vec2(size, size)));
            ui.ctx().request_repaint();
        } else {
            let b = ui.interact(hit, id.with("clear"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            // Rouge au survol : la croix supprime la pièce du suivi.
            let color = if b.hovered() { Color32::from_rgb(230, 85, 70) } else { Color32::LIGHT_GRAY };
            let painter = ui.painter();
            painter.circle_filled(corner, size * 0.55, Color32::from_black_alpha(210));
            painter.circle_stroke(corner, size * 0.55, Stroke::new(if b.hovered() { 2.0_f32 } else { 1.0 }, color));
            painter.text(corner, Align2::CENTER_CENTER, "✕", FontId::proportional(size * 0.7), color);
            clear = b.on_hover_text("Ne plus suivre cette pièce").clicked();
        }
    } else {
        ui.data_mut(|d| d.remove::<f64>(since_id));
    }

    let mut action = if clear { SlotAction::Clear } else { SlotAction::None };
    if let Some(it) = item {
        if hovered {
            resp.clone().on_hover_ui(|ui| {
                // Deux colonnes : la pièce, puis ce que le build recommande à cet emplacement.
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        item_card(ui, it);
                        if let Some(v) = sockets {
                            socket_card(ui, v);
                        }
                    });
                    if let Some(b) = build {
                        ui.separator();
                        ui.vertical(|ui| build_card(ui, b));
                    }
                });
            });
        }
        resp.context_menu(|ui| {
            if ui.button("Ne plus suivre cette pièce").clicked() {
                action = SlotAction::Clear;
                ui.close_menu();
            }
        });
    }
    action
}

/// Châsses d'une pièce suivie, prêtes à afficher : augments posés (nom, icône), châsses
/// vides, suggestions pour le build (déjà mises en forme) et effets non reconnus.
pub struct SocketView {
    /// Augment posé, son icône, et s'il est celui que le build conseille (`None` : le build
    /// ne conseille rien sur cette pièce).
    pub filled: Vec<(String, Option<TextureHandle>, Option<bool>)>,
    pub empty: usize,
    pub unknown: Vec<String>,
    /// Icônes en noir et blanc dans les châsses vides : runes que le build y conseille.
    pub ghosts: Vec<Option<TextureHandle>>,
    /// À défaut, augments qui collent aux objectifs du build.
    pub suggestions: Vec<String>,
}

/// Châsses au milieu de l'image, les unes sous les autres comme en jeu : icône de l'augment
/// posé, ou châsse vide.
fn paint_sockets(painter: &egui::Painter, rect: Rect, v: &SocketView) {
    let n = v.filled.len() + v.unknown.len() + v.empty;
    if n == 0 {
        return;
    }
    // Un écart entre châsses d'un quart de leur taille.
    let size = (rect.width() * 0.34).min(rect.height() * 0.8 / (n as f32 * 1.25)).min(52.0);
    let step = size * 1.25;
    let top = rect.center().y - step * n as f32 / 2.0 + step / 2.0;
    for i in 0..n {
        let c = Pos2::new(rect.center().x, top + i as f32 * step);
        let r = size / 2.0;
        painter.circle_filled(c, r, Color32::from_black_alpha(200));
        match v.filled.get(i) {
            Some((_, tex, good)) => {
                if let Some(tex) = tex {
                    painter.image(tex.id(), Rect::from_center_size(c, Vec2::splat(size * 0.86)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                // Vert : la rune conseillée ; rouge : une autre ; doré : le build ne dit rien.
                let color = match good {
                    Some(true) => SOCKET_GOOD,
                    Some(false) => SOCKET_WRONG,
                    None => GOLD_DIM,
                };
                painter.circle_stroke(c, r, Stroke::new(1.5_f32, color));
            }
            // Effet non reconnu : châsse occupée, sans icône.
            None if i < v.filled.len() + v.unknown.len() => {
                painter.circle_stroke(c, r, Stroke::new(1.5_f32, GOLD_DIM));
            }
            None => {
                let ghost = v.ghosts.get(i - v.filled.len() - v.unknown.len()).cloned().flatten();
                match ghost {
                    Some(tex) => {
                        painter.image(tex.id(), Rect::from_center_size(c, Vec2::splat(size * 0.86)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_gray(150));
                    }
                    None => {
                        painter.circle_filled(c, r * 0.55, Color32::from_black_alpha(230));
                    }
                }
                painter.circle_stroke(c, r, Stroke::new(1.5_f32, SOCKET_EMPTY));
            }
        }
    }
}

pub const SOCKET_EMPTY: Color32 = Color32::from_gray(150);
pub const SOCKET_GOOD: Color32 = Color32::from_rgb(110, 210, 110);
pub const SOCKET_WRONG: Color32 = Color32::from_rgb(230, 85, 70);

/// Châsse vide : à remplir, c'est un gain facile.
pub const EMPTY_SOCKET: Color32 = Color32::from_rgb(230, 150, 70);

/// Partie « châsses » de la fiche d'une pièce suivie.
fn socket_card(ui: &mut egui::Ui, v: &SocketView) {
    let total = v.filled.len() + v.unknown.len() + v.empty;
    if total == 0 {
        return;
    }
    ui.separator();
    ui.label(RichText::new(format!("Châsses : {}/{total} occupées", total - v.empty)).color(Color32::GRAY));
    for (name, _, good) in &v.filled {
        let (mark, color) = match good {
            Some(true) => ("✓", SOCKET_GOOD),
            Some(false) => ("✗", SOCKET_WRONG),
            None => ("◆", Color32::from_rgb(136, 136, 255)),
        };
        ui.label(RichText::new(format!("  {mark} {name}")).color(color));
    }
    for line in &v.unknown {
        ui.label(RichText::new(format!("  ◆ augment non reconnu : {line}")).color(Color32::GRAY));
    }
    if v.empty > 0 {
        ui.label(RichText::new(format!("{} châsse(s) vide(s)", v.empty)).color(EMPTY_SOCKET).strong());
        if !v.suggestions.is_empty() {
            ui.label(RichText::new("Pour ton build :").color(Color32::GRAY).small());
            for s in &v.suggestions {
                ui.label(RichText::new(format!("  {s}")).color(GOLD));
            }
        }
    }
}

/// Ce que le build recommande à un emplacement, comparé à la pièce suivie.
pub struct BuildCard {
    pub step: String,
    pub base: String,
    /// Défenses du type de base visé : armure, évasion, bouclier d'énergie.
    pub base_defences: Option<[f64; 3]>,
    /// Implicites du type de base visé.
    pub implicits: Vec<String>,
    pub unique: Option<String>,
    /// Ligne visée et ce qu'en a la pièce.
    pub lines: Vec<(String, crate::compare::Have)>,
    /// Runes recommandées (déjà mises en forme).
    pub runes: Vec<String>,
    /// Illustration de l'unique ou du type de base recommandé.
    pub image: Option<TextureHandle>,
}

impl BuildCard {
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.unique.is_none() && self.lines.is_empty() && self.runes.is_empty()
    }
}

/// Colonne « recommandé par le build » de la fiche d'une pièce.
fn build_card(ui: &mut egui::Ui, b: &BuildCard) {
    ui.set_max_width(320.0);
    ui.label(RichText::new("RECOMMANDÉ PAR LE BUILD").color(GOLD_DIM).small().strong());
    ui.label(RichText::new(&b.step).color(Color32::GRAY).small());
    ui.add_space(4.0);
    // Illustration de la pièce recommandée, à sa taille en jeu au plus.
    if let Some(tex) = &b.image {
        let size = tex.size_vec2();
        let scale = (110.0 / size.y).min(90.0 / size.x).min(1.0);
        ui.add(egui::Image::new((tex.id(), size * scale)));
        ui.add_space(4.0);
    }
    build_card_body(ui, b);
}

/// Ce que le build recommande (unique, base, lignes visées marquées ✓ ~ ✗, runes), sans titre.
pub fn build_card_body(ui: &mut egui::Ui, b: &BuildCard) {
    build_card_head(ui, b);
    build_card_lines(ui, b);
}

/// Haut de la carte : unique, base et ses défenses, implicites.
pub fn build_card_head(ui: &mut egui::Ui, b: &BuildCard) {
    if let Some(u) = &b.unique {
        ui.label(RichText::new(u).color(rarity_color(Rarity::Unique)).size(15.0).strong());
    }
    if !b.base.is_empty() {
        ui.label(RichText::new(&b.base).color(Color32::LIGHT_GRAY));
    }
    if let Some(d) = b.base_defences {
        let parts: Vec<String> = ["Armour", "Evasion Rating", "Energy Shield"]
            .iter()
            .zip(d)
            .filter(|(_, v)| *v > 0.0)
            .map(|(n, v)| format!("{n} {v:.0}"))
            .collect();
        ui.label(RichText::new(format!("Base : {}", parts.join(" · "))).color(Color32::GRAY).small());
    }
    for line in &b.implicits {
        ui.label(RichText::new(format!("Implicite : {line}")).color(Color32::from_rgb(136, 136, 255)).small());
    }
}

/// Bas de la carte : lignes visées marquées ✓ ~ ✗, runes.
pub fn build_card_lines(ui: &mut egui::Ui, b: &BuildCard) {
    use crate::compare::Have;
    if !b.lines.is_empty() {
        ui.add_space(4.0);
    }
    for (line, have) in &b.lines {
        let (mark, color) = match have {
            Have::Full(_) => ("✓", SOCKET_GOOD),
            Have::Partial(_) => ("~", EMPTY_SOCKET),
            Have::Missing => ("✗", SOCKET_WRONG),
            Have::Unknown => ("•", GOLD_DIM),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(mark).color(color).strong());
            ui.label(RichText::new(line).color(Color32::from_rgb(136, 136, 255)));
        });
    }
    if !b.runes.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Runes :").color(Color32::GRAY).small());
        for r in &b.runes {
            ui.label(RichText::new(format!("  {r}")).color(GOLD));
        }
    }
}

/// Zone de l'étoile d'unique, dans le coin haut gauche d'une case.
fn star_rect(rect: Rect) -> Rect {
    let size = (rect.width().min(rect.height()) * 0.22).clamp(11.0, 15.0);
    Rect::from_center_size(rect.left_top() + egui::vec2(size * 0.55 + 5.0, size * 0.55 + 5.0), egui::vec2(size * 1.1, size * 1.1))
}

/// Zone de la croix « ne plus suivre », dans le coin haut droit : symétrique de l'étoile.
fn clear_rect(rect: Rect) -> Rect {
    let star = star_rect(rect);
    star.translate(egui::vec2(rect.right() - star.right() - (star.left() - rect.left()), 0.0))
}

/// Pointeur sur l'étoile : la case ne montre pas sa propre infobulle.
fn over_star(ui: &egui::Ui, rect: Rect, star: bool) -> bool {
    star && ui.rect_contains_pointer(star_rect(rect))
}

/// Unique recommandé montré dans l'infobulle de l'étoile.
pub struct WantedUnique {
    pub name: String,
    pub image: Option<TextureHandle>,
    /// Prix au marché déjà mis en forme.
    pub price: Option<String>,
}

/// Étoile dans le coin d'une case : le build recommande ici un unique pas encore porté.
/// Renvoie vrai au clic (ouvrir l'onglet Uniques).
pub fn wanted_star(ui: &mut egui::Ui, rect: Rect, slot: Slot,
    uniques: &[WantedUnique], step: &str,
) -> bool {
    let hit = star_rect(rect);
    let (center, size) = (hit.center(), hit.width() / 1.1);
    let resp = ui.interact(hit, ui.id().with(("wanted", slot)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let unique = rarity_color(Rarity::Unique);
    let painter = ui.painter();
    painter.circle_filled(center, size * 0.55, Color32::from_black_alpha(210));
    painter.circle_stroke(center, size * 0.55, Stroke::new(if resp.hovered() { 2.0_f32 } else { 1.0 }, unique));
    painter.text(center, Align2::CENTER_CENTER, "★", FontId::proportional(size * 0.75), unique);
    let clicked = resp.clicked();
    resp.on_hover_ui(|ui| {
        ui.label(RichText::new(format!("Unique recommandé par le build ({step})")).color(Color32::GRAY));
        for u in uniques {
            ui.add_space(4.0);
            // Icône à gauche, nom et prix au marché à droite.
            ui.horizontal(|ui| {
                if let Some(tex) = &u.image {
                    let size = tex.size_vec2();
                    let scale = (110.0 / size.y).min(90.0 / size.x).min(1.0);
                    ui.add(egui::Image::new((tex.id(), size * scale)));
                }
                ui.vertical(|ui| {
            ui.label(RichText::new(&u.name).color(unique).size(15.0).strong());
                    match &u.price {
                        Some(p) => {
                            ui.label(RichText::new(p).color(Color32::from_rgb(225, 220, 210)))
                        }
                        None => {
                            ui.label(RichText::new("pas de prix au marché").color(Color32::GRAY))
                        }
                    };
                });
            });
        }
        ui.add_space(4.0);
        ui.label(RichText::new("clic : onglet Uniques (prix, recherche sur le trade)").color(Color32::GRAY).small());
    });
    clicked
}

/// Fiche d'objet façon infobulle du jeu.
pub fn item_card(ui: &mut egui::Ui, it: &Item) {
    ui.set_max_width(360.0);
    let color = rarity_color(it.rarity);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(&it.name).color(color).size(16.0).strong());
        if it.base != it.name {
            ui.label(RichText::new(&it.base).color(color).size(14.0));
        }
    });
    ui.separator();
    let mut header = Vec::new();
    for key in ["Armour", "Evasion Rating", "Energy Shield", "Ward", "Spirit"] {
        if let Some(v) = it.props.get(key) {
            header.push(format!("{key}: {v}"));
        }
    }
    if let (Some(r), Some(t)) = (it.props.get("recover"), it.props.get("recover_time")) {
        header.push(format!("Récupère {r} en {t} s"));
    }
    if let Some(d) = it.props.get("duration") {
        header.push(format!("Dure {d} s"));
    }
    if let (Some(u), Some(m)) = (it.props.get("charges_use"), it.props.get("charges_max")) {
        header.push(format!("{u} charges sur {m} par utilisation"));
    }
    for h in &header {
        ui.label(RichText::new(h).color(Color32::from_rgb(200, 200, 200)));
    }
    if it.req_level > 0 || it.item_level > 0 {
        ui.label(RichText::new(format!("Niveau requis {} · niveau d'objet {}", it.req_level, it.item_level)).color(DIM));
    }
    if !it.granted_skills.is_empty() {
        ui.separator();
        for s in &it.granted_skills {
            ui.label(RichText::new(format!("Compétence : {s}")).color(Color32::from_rgb(200, 200, 200)));
        }
    }
    if !it.mods.is_empty() {
        ui.separator();
        for m in &it.mods {
            ui.label(RichText::new(m).color(MOD_COLOR));
        }
    }
    if it.unidentified {
        ui.label(RichText::new("Non identifié").color(Color32::from_rgb(210, 0, 0)));
    }
    if it.corrupted {
        ui.label(RichText::new("Corrompu").color(Color32::from_rgb(210, 0, 0)));
    }
}

pub fn verdict_style(v: crate::compare::Verdict) -> (&'static str, Color32) {
    use crate::compare::Verdict;
    match v {
        Verdict::Better => ("▲ MIEUX", Color32::from_rgb(90, 200, 90)),
        Verdict::Worse => ("▼ MOINS BIEN", Color32::from_rgb(225, 80, 70)),
        Verdict::Equal => ("= ÉQUIVALENT", Color32::from_rgb(170, 170, 170)),
        Verdict::Unknown => ("? À VOIR", Color32::from_rgb(225, 170, 60)),
        Verdict::Same => ("● ÉQUIPÉ", Color32::from_rgb(100, 150, 230)),
    }
}

pub fn value_color(l: crate::rules::Label) -> Color32 {
    use crate::rules::Label;
    match l {
        Label::Keep => Color32::from_rgb(90, 200, 90),
        Label::Market => Color32::from_rgb(235, 200, 60),
        Label::Check => Color32::from_rgb(80, 200, 220),
        Label::Vendor => Color32::from_rgb(140, 140, 140),
    }
}

/// Dégradé vers la couleur du panneau sur le bas de `rect`, sur `height` pixels. Une zone qui
/// défile laisse dépasser son contenu de `clip_rect_margin` sous son bord : le fondu le couvre.
fn fade_bottom_on(ui: &egui::Ui, rect: Rect, height: f32, bg: Color32) {
    let bottom = rect.bottom() + ui.visuals().clip_rect_margin + 1.0;
    let band = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - height), Pos2::new(rect.right(), bottom));
    let mut mesh = egui::Mesh::default();
    let clear = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 0);
    mesh.colored_vertex(band.left_top(), clear);
    mesh.colored_vertex(band.right_top(), clear);
    mesh.colored_vertex(Pos2::new(band.right(), rect.bottom()), bg);
    mesh.colored_vertex(Pos2::new(band.left(), rect.bottom()), bg);
    mesh.colored_vertex(band.right_bottom(), bg);
    mesh.colored_vertex(band.left_bottom(), bg);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh.add_triangle(3, 2, 4);
    mesh.add_triangle(3, 4, 5);
    ui.painter().with_clip_rect(band).add(egui::Shape::mesh(mesh));
}

/// Fondu sous le titre fixe d'une zone qui a défilé, et au bas de la zone (`bottom`) tant qu'il
/// reste du contenu en dessous : pas de ligne coupée net.
pub fn fade_scroll<R>(ui: &egui::Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, bottom: bool) {
    fade_scroll_on(ui, out, bottom, ui.visuals().panel_fill);
}

/// Pareil, sur un fond de couleur `bg` (carte).
pub fn fade_scroll_on<R>(ui: &egui::Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, bottom: bool, bg: Color32) {
    let rest = out.content_size.y - out.state.offset.y - out.inner_rect.height();
    if bottom && rest > 1.0 {
        fade_bottom_on(ui, out.inner_rect, 40.0, bg);
    }
    if out.state.offset.y > 1.0 {
        fade_top_on(ui, out.inner_rect, 28.0, bg);
    }
}

/// Dégradé depuis la couleur du panneau sur le haut de `rect`, sur `height` pixels (même marge
/// de débordement qu'en bas).
fn fade_top_on(ui: &egui::Ui, rect: Rect, height: f32, bg: Color32) {
    let top = rect.top() - ui.visuals().clip_rect_margin - 1.0;
    let band = Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(rect.right(), rect.top() + height));
    let clear = Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(band.left_top(), bg);
    mesh.colored_vertex(band.right_top(), bg);
    mesh.colored_vertex(Pos2::new(band.right(), rect.top()), bg);
    mesh.colored_vertex(Pos2::new(band.left(), rect.top()), bg);
    mesh.colored_vertex(band.right_bottom(), clear);
    mesh.colored_vertex(band.left_bottom(), clear);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh.add_triangle(3, 2, 4);
    mesh.add_triangle(3, 4, 5);
    ui.painter().with_clip_rect(band).add(egui::Shape::mesh(mesh));
}

/// Titre d'une zone du diagnostic, avec un complément à droite (nom, étape…).
pub fn zone_title(ui: &mut egui::Ui, title: &str, detail: &str) -> egui::Response {
    ui.add_space(8.0);
    ui.separator();
    let resp = ui
        .horizontal_wrapped(|ui| {
            ui.label(RichText::new(title).color(GOLD_DIM).small().strong());
            if !detail.is_empty() {
                ui.label(RichText::new(detail).color(Color32::GRAY).small());
            }
        })
        .response;
    ui.add_space(2.0);
    resp
}

/// Badge compact (verdict dans le coin de l'aperçu du diagnostic).
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    // Fond opaque : le badge peut flotter par-dessus du contenu qui défile.
    egui::Frame::new()
        .fill(ui.visuals().panel_fill.lerp_to_gamma(color, 0.3))
        .stroke(Stroke::new(1.5_f32, color))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color.lerp_to_gamma(Color32::WHITE, 0.25)).size(17.0).strong());
        });
}

/// Titre de section d'un menu (petites capitales grises).
pub fn menu_section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(11.0).color(Color32::from_gray(120)));
    ui.add_space(2.0);
}

/// Entrée de menu sur toute la largeur : icône, libellé, précision en gris dessous ; surlignée
/// au survol, sans cadre de bouton. `danger` : action destructive, en rouge.
pub fn menu_item(ui: &mut egui::Ui, icon: &str, label: &str, hint: &str, enabled: bool, danger: bool) -> egui::Response {
    let width = ui.available_width().max(240.0);
    let height = if hint.is_empty() { 30.0 } else { 44.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter();
    if enabled && resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(14));
    }
    let base = if !enabled {
        Color32::from_gray(90)
    } else if danger {
        Color32::from_rgb(225, 110, 95)
    } else {
        Color32::from_gray(225)
    };
    let left = rect.left() + 10.0;
    let top = rect.top() + if hint.is_empty() { rect.height() / 2.0 } else { 14.0 };
    painter.text(Pos2::new(left + 9.0, top), Align2::CENTER_CENTER, icon, FontId::proportional(15.0), if danger || !enabled { base } else { GOLD });
    painter.text(Pos2::new(left + 28.0, top), Align2::LEFT_CENTER, label, FontId::proportional(15.0), base);
    if !hint.is_empty() {
        painter.text(Pos2::new(left + 28.0, top + 17.0), Align2::LEFT_CENTER, hint, FontId::proportional(12.0), Color32::from_gray(if enabled { 125 } else { 80 }));
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Couleur d'un niveau de confiance (en %) : vert, orange, rouge.
pub fn confidence_color(pct: u32) -> Color32 {
    match pct {
        75.. => Color32::from_rgb(110, 200, 110),
        50.. => Color32::from_rgb(225, 160, 70),
        _ => Color32::from_rgb(220, 95, 85),
    }
}

pub const GOLD: Color32 = Color32::from_rgb(200, 172, 112);
/// Fond des cartes (route, activité) : un cran plus clair que les panneaux.
pub const CARD_FILL: Color32 = Color32::from_rgb(20, 18, 15);
pub const GOLD_DIM: Color32 = Color32::from_rgb(120, 100, 64);

/// Listes et boutons sombres à liseré doré (barre posée sur le fond du jeu).
pub fn gold_widgets(ui: &mut egui::Ui) {
    let v = &mut ui.style_mut().visuals;
    let dark = Color32::from_rgb(18, 15, 12);
    for (w, stroke) in [
        (&mut v.widgets.inactive, GOLD_DIM),
        (&mut v.widgets.hovered, GOLD),
        (&mut v.widgets.active, GOLD),
        (&mut v.widgets.open, GOLD),
    ] {
        w.weak_bg_fill = dark;
        w.bg_fill = dark;
        w.bg_stroke = Stroke::new(1.0_f32, stroke);
        w.corner_radius = CornerRadius::same(3);
    }
    ui.style_mut().spacing.button_padding = egui::vec2(8.0, 3.0);
}

/// Bouton d'action (« 💰  Estimer ») dans le style des boutons de la barre du bas (Menu,
/// Regex). Le survol est lu sur le pointeur, le bouton pouvant être sous une ligne cliquable.
pub fn action_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui
        .scope(|ui| {
            gold_widgets(ui);
            ui.add(egui::Button::new(RichText::new(text).color(GOLD)))
        })
        .inner;
    if ui.rect_contains_pointer(resp.rect) {
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(3),
            Stroke::new(1.0_f32, GOLD),
            StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Rangée d'onglets dans le style des titres de section : l'actif en doré, souligné, posé sur
/// un filet doré qui court sous toute la rangée. Renvoie l'onglet cliqué.
pub fn tab_strip<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    rect: Rect,
    tabs: &[(T, &str)],
    current: T,
) -> Option<T> {
    let line_y = rect.bottom() - 1.0;
    ui.painter().hline(
        rect.x_range(),
        line_y,
        Stroke::new(1.0_f32, GOLD_DIM.gamma_multiply(0.6)),
    );
    let font = FontId::proportional(14.0);
    let mut x = rect.left() + 6.0;
    let mut picked = None;
    for (tab, label) in tabs {
        let on = current == *tab;
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE);
        let r = Rect::from_min_max(
            egui::pos2(x, rect.top() + 3.0),
            egui::pos2(x + galley.size().x + 24.0, line_y),
        );
        let resp = ui
            .interact(r, ui.id().with(("tab", *label)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter();
        if on {
            painter.rect_filled(
                r,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                GOLD.gamma_multiply(0.10),
            );
            painter.hline(r.x_range(), line_y - 1.0, Stroke::new(2.0_f32, GOLD));
        } else if resp.hovered() {
            painter.rect_filled(
                r,
                CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                Color32::from_white_alpha(6),
            );
        }
        let color = if on {
            GOLD
        } else if resp.hovered() {
            Color32::LIGHT_GRAY
        } else {
            Color32::GRAY
        };
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            *label,
            font.clone(),
            color,
        );
        if resp.clicked() {
            picked = Some(*tab);
        }
        x = r.right() + 4.0;
    }
    picked
}

/// Titre de section : petites capitales dorées, compteur, actions discrètes à droite, filet dessous.
pub fn section_header(ui: &mut egui::Ui, title: &str, count: Option<usize>, actions: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).color(GOLD).size(13.0).strong());
        if let Some(n) = count {
            ui.label(RichText::new(n.to_string()).color(GOLD_DIM).size(12.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
    });
    ui.add_space(2.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, GOLD_DIM.gamma_multiply(0.6)));
    ui.add_space(8.0);
}

/// Rend aux widgets le style de l'application (après `gold_widgets`) : les barres de
/// défilement en prennent leurs couleurs.
pub fn plain_widgets(ui: &mut egui::Ui) {
    let widgets = ui.ctx().style().visuals.widgets.clone();
    ui.style_mut().visuals.widgets = widgets;
}

/// Titre de section suivi d'un sous-titre (résumé) sur la même ligne : son trait reste aligné
/// avec celui des sections voisines.
pub fn section_header_sub(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).color(GOLD).size(13.0).strong());
        ui.add(egui::Label::new(RichText::new(subtitle).color(Color32::GRAY).small()).truncate()).on_hover_text(subtitle);
    });
    ui.add_space(2.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, GOLD_DIM.gamma_multiply(0.6)));
    ui.add_space(8.0);
}

/// Petite courbe de tendance (variations jour par jour).
pub fn sparkline(ui: &mut egui::Ui, data: &[f64], color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 18.0), Sense::hover());
    if data.len() < 2 {
        return;
    }
    let (lo, hi) = data
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
    let span = (hi - lo).max(1e-9);
    let points: Vec<Pos2> = data
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + rect.width() * i as f32 / (data.len() - 1) as f32;
            let y = rect.bottom() - rect.height() * ((v - lo) / span) as f32;
            egui::pos2(x, y)
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        Stroke::new(1.2_f32, color.gamma_multiply(0.8)),
    ));
}

/// Combinaison de touches dessinée comme sur un clavier : « [Ctrl] + [C] ».
pub fn keys(ui: &mut egui::Ui, keys: &[&str]) {
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            ui.label(RichText::new("+").color(GOLD_DIM).small());
        }
        egui::Frame::new()
            .fill(Color32::from_rgb(28, 25, 21))
            .stroke(Stroke::new(1.0_f32, GOLD_DIM))
            .corner_radius(3)
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.label(RichText::new(*k).color(Color32::LIGHT_GRAY).small().strong());
            });
    }
}

/// Action discrète sous forme de texte (pas de cadre de bouton).
pub fn text_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui.add(egui::Label::new(RichText::new(text).color(Color32::GRAY).size(12.0)).sense(Sense::click()));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Fenêtre modale : fond assombri, clics bloqués derrière, titre doré et croix de fermeture.
/// Renvoie le résultat du contenu et `true` si on l'a fermée (croix, Échap, clic à côté).
pub fn modal<R>(ctx: &egui::Context, id: &str, title: &str, closable: bool, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> (R, bool) {
    let frame = egui::Frame::window(&ctx.style())
        .fill(Color32::from_rgb(12, 11, 10))
        .stroke(Stroke::new(1.0_f32, GOLD_DIM))
        .corner_radius(6)
        .inner_margin(16);
    let resp = egui::Modal::new(egui::Id::new(id)).backdrop_color(Color32::from_black_alpha(170)).frame(frame).show(ctx, |ui| {
        ui.set_width(width);
        gold_widgets(ui);
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).color(GOLD).size(17.0).strong());
            if closable {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| close = text_button(ui, "✕").clicked());
            }
        });
        ui.add_space(2.0);
        ui.separator();
        ui.add_space(4.0);
        (add(ui), close)
    });
    let should_close = resp.should_close();
    let (inner, close) = resp.inner;
    (inner, close || (closable && should_close))
}
