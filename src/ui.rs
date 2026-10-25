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
        Slot::Weapon => [132.0, 98.0, 220.0, 433.0],
        Slot::Helmet => [543.0, 73.0, 224.0, 220.0],
        Slot::Offhand => [956.0, 98.0, 225.0, 433.0],
        Slot::Body => [543.0, 308.0, 224.0, 327.0],
        Slot::Amulet => [802.0, 283.0, 118.0, 117.0],
        Slot::RingLeft => [389.0, 415.0, 117.0, 117.0],
        Slot::RingRight => [802.0, 415.0, 118.0, 117.0],
        Slot::Gloves => [283.0, 546.0, 223.0, 224.0],
        Slot::Boots => [802.0, 546.0, 225.0, 224.0],
        Slot::Belt => [543.0, 650.0, 224.0, 120.0],
        Slot::LifeFlask => [338.0, 775.0, 104.0, 206.0],
        Slot::Charm1 => [491.0, 825.0, 104.0, 104.0],
        Slot::Charm2 => [597.0, 825.0, 104.0, 104.0],
        Slot::Charm3 => [703.0, 825.0, 104.0, 104.0],
        Slot::ManaFlask => [858.0, 775.0, 104.0, 206.0],
    }
}
/// Bandeau vide en haut de l'image de fond, entre ses deux ornements d'angle :
/// emplacement de la barre personnage / build.
/// Mesuré sur l'image : zone noire de 40 px de haut, ornements d'angle sur ~20 px.
pub const HEADER_STRIP: [f32; 4] = [34.0, 0.0, 1252.0, 40.0];
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
pub fn blocked_slot(ui: &mut egui::Ui, rect: Rect, slot: Slot, why: Blocked, framed: bool) {
    let resp = ui.interact(rect, ui.id().with(("blocked", slot)), Sense::hover());
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
            resp.on_hover_text("Occupée par ton arme à deux mains");
        }
        Blocked::ClosedCharm(blocker) => {
            if let Some(tex) = blocker {
                paint_texture(painter, rect, tex, Color32::WHITE);
            }
            resp.on_hover_text("Ta ceinture n'ouvre pas cet emplacement de charme");
        }
    }
}

fn paint_texture(painter: &egui::Painter, rect: Rect, tex: &TextureHandle, tint: Color32) {
    let inner = rect.shrink(4.0);
    let size = tex.size_vec2();
    let scale = (inner.width() / size.x).min(inner.height() / size.y);
    let img = Rect::from_center_size(inner.center(), size * scale);
    painter.image(tex.id(), img, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
}

/// Une case de la grille. `glow` colore le cadre (case comparée, phase 2).
pub fn slot(
    ui: &mut egui::Ui,
    rect: Rect,
    slot: Slot,
    item: Option<&Item>,
    texture: Option<&TextureHandle>,
    glow: Option<Color32>,
    framed: bool,
) -> SlotAction {
    let id = ui.id().with(("slot", slot));
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    // Sur le fond du jeu, on masque la silhouette de l'emplacement sous un objet suivi.
    if framed || item.is_some() {
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
        (Some(_), Some(tex)) => paint_texture(painter, rect, tex, Color32::WHITE),
        (Some(it), None) => {
            painter.text(rect.center(), Align2::CENTER_CENTER, &it.name, FontId::proportional(if small { 8.0 } else { 11.0 }), rarity_color(it.rarity));
        }
        (None, _) => {
            if framed && !small {
                painter.text(rect.center(), Align2::CENTER_CENTER, slot.label(), FontId::proportional(11.0), DIM);
            }
            if resp.hovered() {
                resp.clone().on_hover_text(format!("{} : non suivi — Ctrl+Alt+C sur ta pièce en jeu", slot.label()));
            }
        }
    }

    let mut action = SlotAction::None;
    if let Some(it) = item {
        resp.clone().on_hover_ui(|ui| item_card(ui, it));
        resp.context_menu(|ui| {
            if ui.button("Ne plus suivre cette pièce").clicked() {
                action = SlotAction::Clear;
                ui.close_menu();
            }
        });
    }
    action
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

/// Grand badge coloré (verdict de comparaison ou valeur marchande).
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.5_f32, color))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(18.0).strong());
        });
}

pub const GOLD: Color32 = Color32::from_rgb(200, 172, 112);
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
