//! Dessin de la grille d'équipement (disposition de l'écran de personnage de PoE2)
//! et des fiches d'objets façon infobulle du jeu.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, Vec2,
};

use crate::gametext;
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
            (true, _) => tr!("Un unique du build manque dans ce set"),
            (false, true) => tr!("Passifs de ce set entourés de sa couleur"),
            (false, false) => tr!("Set d'armes affiché"),
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

/// Petites cases du set inactif, aux extrémités comme en jeu (le fond ne les dessine pas : elles
/// ont leur propre cadre) : l'arme à gauche, la main gauche à droite, dans la pierre libre de
/// part et d'autre des grandes cases.
pub fn mini_layout(slot: Slot) -> [f32; 4] {
    match slot.in_set(1) {
        Slot::Offhand => [1189.0, 98.0, 82.0, 196.0],
        _ => [41.0, 98.0, 82.0, 196.0],
    }
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

pub const MOD_COLOR: Color32 = Color32::from_rgb(136, 136, 255);
const DIM: Color32 = Color32::from_rgb(127, 127, 127);
const SLOT_FILL: Color32 = Color32::from_rgb(22, 20, 18);
const SLOT_EMPTY_STROKE: Color32 = Color32::from_rgb(70, 62, 50);

pub enum SlotAction {
    None,
    Clear,
    /// Clic sur la case : ouvrir son détail (portée face à la recommandation, prix, trade).
    Open,
}

/// Cases qui ne peuvent pas recevoir de pièce.
pub enum Blocked<'a> {
    /// Main gauche prise par une arme à deux mains (on la montre en transparence).
    ByTwoHander(Option<&'a TextureHandle>),
    /// Emplacement de charme que la ceinture n'ouvre pas (icône de verrou du jeu si disponible).
    ClosedCharm(Option<&'a TextureHandle>),
}

/// `framed` : dessiner nos propres cadres (quand le fond du jeu n'est pas disponible).
/// Renvoie vrai au clic : le détail de la case reste consultable (ce que le build y prévoit).
pub fn blocked_slot(ui: &mut egui::Ui, rect: Rect, slot: Slot, why: Blocked, framed: bool, star: bool) -> bool {
    let resp = ui.interact(rect, ui.id().with(("blocked", slot)), Sense::click());
    let clicked = resp.clicked();
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
                resp.on_hover_text(tr!("Occupée par ton arme à deux mains"));
            }
        }
        Blocked::ClosedCharm(blocker) => {
            if let Some(tex) = blocker {
                paint_texture(painter, rect, tex, Color32::WHITE);
            }
            if hovered {
                resp.on_hover_text(tr!("Ta ceinture n'ouvre pas cet emplacement de charme (clic : ce que le build y prévoit)"));
            }
        }
    }
    clicked
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
    worn: Option<&WornView>,
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
                            ui.label(RichText::new(tr!("{slot} : non suivi — Ctrl+Alt+C sur ta pièce en jeu", slot = slot.label())).color(Color32::GRAY).small());
                            ui.separator();
                            build_card(ui, b);
                        });
                    }
                    None => {
                        resp.clone().on_hover_text(tr!("{slot} : non suivi — Ctrl+Alt+C sur ta pièce en jeu", slot = slot.label()));
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
            clear = b.on_hover_text(tr!("Ne plus suivre cette pièce")).clicked();
        }
    } else {
        ui.data_mut(|d| d.remove::<f64>(since_id));
    }

    let mut action = if clear {
        SlotAction::Clear
    } else if resp.clicked() {
        SlotAction::Open
    } else {
        SlotAction::None
    };
    // Rune survolée : sa fiche (effets), à la place de celle de la pièce.
    let rune = if hovered { sockets.and_then(|v| hovered_socket(ui, rect, v)) } else { None };
    if let Some(r) = rune {
        resp.clone().on_hover_ui(|ui| rune_tip(ui, r));
    } else if item.is_some() && hovered {
        // Même rendu que la colonne « actuellement équipé » du détail (clic).
        if let Some(w) = worn {
            resp.clone().on_hover_ui(|ui| {
                // Un peu d'air autour du contenu ; largeur du contenu (le bandeau du bas s'y étend),
                // assez pour le bandeau lui-même.
                egui::Frame::new().inner_margin(8).show(ui, |ui| {
                    ui.set_width(worn_width(ui, w).max(300.0).min(520.0));
                    worn_column(ui, w);
                    click_hint(ui, tr!("Clic : comparer avec le build"), w.score);
                });
            });
        }
    }
    if item.is_some() {
        resp.context_menu(|ui| {
            if ui.button(tr!("Ne plus suivre cette pièce")).clicked() {
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
    /// Fiche de chaque augment posé (même ordre que `filled`), au survol de sa châsse.
    pub tips: Vec<RuneLine>,
}

/// Centre et rayon de chaque châsse d'une case, les unes sous les autres au milieu de l'image.
fn socket_circles(rect: Rect, v: &SocketView) -> Vec<(Pos2, f32)> {
    let n = v.filled.len() + v.unknown.len() + v.empty;
    if n == 0 {
        return vec![];
    }
    // Un écart entre châsses d'un quart de leur taille.
    let size = (rect.width() * 0.34).min(rect.height() * 0.8 / (n as f32 * 1.25)).min(52.0);
    let step = size * 1.25;
    let top = rect.center().y - step * n as f32 / 2.0 + step / 2.0;
    (0..n).map(|i| (Pos2::new(rect.center().x, top + i as f32 * step), size / 2.0)).collect()
}

/// Rune posée sous le pointeur, s'il y en a une : sa fiche.
fn hovered_socket<'a>(ui: &egui::Ui, rect: Rect, v: &'a SocketView) -> Option<&'a RuneLine> {
    let pointer = ui.ctx().pointer_hover_pos()?;
    let i = socket_circles(rect, v).iter().position(|(c, r)| c.distance(pointer) <= *r)?;
    v.tips.get(i)
}

/// Une rune (ou un augment) sur sa ligne : icône, marque éventuelle, nom, puis le reste
/// (prix, effets). Une ligne par exemplaire, jamais « ×2 ».
#[derive(Clone)]
pub struct RuneLine {
    pub image: Option<TextureHandle>,
    pub mark: Option<(&'static str, Color32)>,
    pub name: String,
    /// Suite de la ligne, déjà mise en forme (« ≈ 3 ex », « : +10% … ≈ 3 ex »).
    pub rest: String,
    pub color: Color32,
    /// Pour l'infobulle : sorte et niveau (« Rune · niveau 50 »), effets par catégorie d'objet.
    pub kind: String,
    pub effects: Vec<(String, Vec<String>)>,
}

/// Lignes de runes : icône du jeu en tête, à la hauteur du texte.
pub fn rune_lines(ui: &mut egui::Ui, lines: &[RuneLine]) {
    for (i, r) in lines.iter().enumerate() {
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Verdict face au build en tête de ligne, comme pour les mods.
            if let Some((mark, color)) = r.mark {
                ui.add(egui::Label::new(RichText::new(mark).color(color).strong()).selectable(false));
            }
            let h = ui.text_style_height(&egui::TextStyle::Body) + 2.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(h, h), Sense::hover());
            if let Some(tex) = &r.image {
                let s = tex.size_vec2();
                let k = (h / s.x).min(h / s.y);
                ui.painter().image(tex.id(), Rect::from_center_size(rect.center(), s * k), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            // Nom dans sa couleur, prix (et effets) dans celle des prix du reste de l'app.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add(egui::Label::new(RichText::new(gametext::name(&r.name)).color(r.color)).selectable(false));
            if !r.rest.is_empty() {
                ui.add(egui::Label::new(RichText::new(&r.rest).color(Color32::from_rgb(225, 220, 210))).selectable(false));
            }
        });
        // Infobulle sur toute la ligne (icône et texte), posée par-dessus.
        ui.interact(row.response.rect, ui.id().with(("rune_line", i, &r.name)), Sense::hover()).on_hover_ui(|ui| rune_tip(ui, r));
    }
}

/// Fiche d'une rune : grande icône, nom, sorte et niveau, effets selon l'objet qui la porte.
fn rune_tip(ui: &mut egui::Ui, r: &RuneLine) {
    // Même marge que l'infobulle des pièces.
    egui::Frame::new().inner_margin(8).show(ui, |ui| rune_tip_body(ui, r));
}

fn rune_tip_body(ui: &mut egui::Ui, r: &RuneLine) {
    ui.set_max_width(360.0);
    ui.horizontal(|ui| {
        if let Some(tex) = &r.image {
            let s = tex.size_vec2();
            let k = (64.0 / s.x).min(64.0 / s.y);
            ui.add(egui::Image::new((tex.id(), s * k)));
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(gametext::name(&r.name)).color(GOLD).size(16.0).strong());
            if !r.kind.is_empty() {
                ui.label(RichText::new(&r.kind).color(Color32::GRAY));
            }
        });
    });
    for (category, lines) in &r.effects {
        ui.add_space(4.0);
        ui.label(RichText::new(category).color(Color32::GRAY).small());
        for l in lines {
            ui.label(RichText::new(gametext::line(l)).color(MOD_COLOR));
        }
    }
    if r.effects.is_empty() {
        ui.label(RichText::new(tr!("Effets inconnus (catalogue pas encore chargé)")).color(Color32::GRAY).small());
    }
}

/// Châsses au milieu de l'image, les unes sous les autres comme en jeu : icône de l'augment
/// posé, ou châsse vide.
fn paint_sockets(painter: &egui::Painter, rect: Rect, v: &SocketView) {
    for (i, (c, r)) in socket_circles(rect, v).into_iter().enumerate() {
        let size = r * 2.0;
        painter.circle_filled(c, r, Color32::from_black_alpha(200));
        match v.filled.get(i) {
            Some((_, tex, _)) => {
                if let Some(tex) = tex {
                    painter.image(tex.id(), Rect::from_center_size(c, Vec2::splat(size * 0.86)), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                // Contour neutre : la comparaison avec les runes du build est dans le détail.
                painter.circle_stroke(c, r, Stroke::new(1.5_f32, GOLD_DIM));
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
    /// Runes recommandées, une ligne par exemplaire.
    pub runes: Vec<RuneLine>,
    /// Illustration de l'unique ou du type de base recommandé.
    pub image: Option<TextureHandle>,
}

impl BuildCard {
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.unique.is_none() && self.lines.is_empty() && self.runes.is_empty()
    }
}

/// Colonne « recommandé par le build » de la fiche d'une pièce.
pub fn build_card(ui: &mut egui::Ui, b: &BuildCard) {
    ui.set_max_width(320.0);
    ui.label(RichText::new(tr!("RECOMMANDÉ PAR LE BUILD")).color(GOLD_DIM).small().strong());
    ui.label(RichText::new(crate::guide::step_text(&b.step)).color(Color32::GRAY).small());
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
        let parts: Vec<String> = [tr!("Armure"), tr!("Score d'Évasion"), tr!("Bouclier d'énergie")]
            .iter()
            .zip(d)
            .filter(|(_, v)| *v > 0.0)
            .map(|(n, v)| format!("{n} {v:.0}"))
            .collect();
        ui.label(RichText::new(tr!("Base : {defences}", defences = parts.join(" · "))).color(Color32::GRAY).small());
    }
    for line in &b.implicits {
        ui.label(RichText::new(tr!("Implicite : {line}", line)).color(Color32::from_rgb(136, 136, 255)).small());
    }
}

/// Bas de la carte : lignes visées marquées ✓ ~ ✗, runes.
pub fn build_card_lines(ui: &mut egui::Ui, b: &BuildCard) {
    if !b.lines.is_empty() {
        ui.add_space(4.0);
    }
    target_lines(ui, &b.lines);
    if !b.runes.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(tr!("Runes :")).color(Color32::GRAY).small());
        rune_lines(ui, &b.runes);
    }
}

/// Lignes visées par le build, marquées ✓ ~ ✗ selon ce qu'en a la pièce.
pub fn target_lines(ui: &mut egui::Ui, lines: &[(String, crate::compare::Have)]) {
    use crate::compare::Have;
    for (line, have) in lines {
        let (mark, color) = match have {
            Have::Full(_) => ("✓", SOCKET_GOOD),
            Have::Partial(_) => ("~", EMPTY_SOCKET),
            Have::Missing => ("✗", SOCKET_WRONG),
            Have::Unknown => ("•", GOLD_DIM),
        };
        // Un seul texte (marque puis ligne) : même interligne qu'une simple liste de mods.
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut job = egui::text::LayoutJob::default();
        job.append(&format!("{mark}  "), 0.0, egui::TextFormat::simple(font.clone(), color));
        job.append(&gametext::line(line), 0.0, egui::TextFormat::simple(font, Color32::from_rgb(136, 136, 255)));
        ui.label(job);
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

/// Unique recommandé montré dans l'infobulle de l'étoile : la même fiche qu'une pièce portée
/// (en-tête, mods), puis son prix au marché.
pub struct WantedUnique {
    pub view: WornView,
    /// Prix (« ≈ 5 div ») et sa couleur, puis sur quoi il repose (« 2528 annonces… »).
    pub price: (String, Color32),
    pub basis: Option<String>,
}

/// Étoile dans le coin d'une case : le build recommande ici un unique pas encore porté.
/// Renvoie vrai au clic (ouvrir l'onglet Uniques).
pub fn wanted_star(ui: &mut egui::Ui, rect: Rect, slot: Slot,
    uniques: &[&WantedUnique], step: &str,
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
    // Même fiche que le survol d'une pièce, pour l'unique attendu, et son prix en bas.
    resp.on_hover_ui(|ui| wanted_tip(ui, uniques, step));
    clicked
}

/// Icône de nœud d'arbre dans un disque : l'image couvre tout le disque, ce qui dépasse n'est
/// pas dessiné (certaines icônes du jeu sont carrées, d'autres déjà rondes).
pub fn circle_image(painter: &egui::Painter, tex: egui::TextureId, center: Pos2, radius: f32, tint: Color32) {
    const SEGMENTS: usize = 32;
    let mut mesh = egui::Mesh::with_texture(tex);
    mesh.vertices.push(egui::epaint::Vertex { pos: center, uv: Pos2::new(0.5, 0.5), color: tint });
    for k in 0..=SEGMENTS {
        let a = k as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let (sin, cos) = a.sin_cos();
        // Le disque est inscrit dans l'image : ses bords touchent ceux de l'image.
        mesh.vertices.push(egui::epaint::Vertex { pos: center + egui::vec2(cos, sin) * radius, uv: Pos2::new(0.5 + 0.5 * cos, 0.5 + 0.5 * sin), color: tint });
        if k > 0 {
            mesh.add_triangle(0, k as u32, k as u32 + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Fiche d'un nœud d'arbre (talents, Atlas) : icône éventuelle, titre et précision en gris, lignes
/// d'effets, note en petit. Largeur et polices fixes, calculées sur le contenu (au plus 420 px) :
/// la fiche ne dépend ni du zoom ni de la place laissée par le bord de l'écran.
pub fn node_tip(ui: &mut egui::Ui, icon: Option<&TextureHandle>, title: &str, color: Color32, after: Option<&str>, lines: &[String], note: Option<&str>) {
    let title = gametext::name(title);
    let title = title.as_ref();
    let after = after.map(gametext::name);
    let after = after.as_deref();
    let lines: Vec<String> = lines.iter().map(|l| gametext::line(l).into_owned()).collect();
    let text_w = |t: &str, size: f32| ui.fonts(|f| f.layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::WHITE).size().x);
    let head = icon.map_or(0.0, |_| 48.0) + text_w(title, 16.0) + after.map_or(0.0, |a| 8.0 + text_w(a, 14.0));
    let body = lines.iter().map(|l| text_w(l, 14.0)).chain(note.map(|n| text_w(n, 12.0))).fold(head, f32::max);
    egui::Frame::new().inner_margin(8).show(ui, |ui| {
        ui.set_width(body.ceil().clamp(120.0, 420.0));
        ui.horizontal(|ui| {
            if let Some(tex) = icon {
                ui.image((tex.id(), Vec2::splat(40.0)));
            }
            ui.label(RichText::new(title).color(color).size(16.0).strong());
            if let Some(a) = after {
                ui.label(RichText::new(a).color(Color32::GRAY).size(14.0));
            }
        });
        for l in &lines {
            ui.label(RichText::new(l).color(MOD_COLOR).size(14.0));
        }
        if let Some(n) = note {
            ui.add_space(2.0);
            ui.label(RichText::new(n).color(Color32::GRAY).size(12.0));
        }
    });
}

/// Fiche des uniques attendus sur une case (infobulle de l'étoile).
pub fn wanted_tip(ui: &mut egui::Ui, uniques: &[&WantedUnique], step: &str) {
    egui::Frame::new().inner_margin(8).show(ui, |ui| {
        let width = uniques.iter().map(|u| worn_width(ui, &u.view)).fold(300.0_f32, f32::max).min(520.0);
        ui.set_width(width);
        ui.label(RichText::new(tr!("UNIQUE ATTENDU · {step}", step)).color(GOLD_DIM).small().strong());
        ui.add_space(4.0);
        for (i, u) in uniques.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            worn_header(ui, &format!("wanted_{i}"), &u.view);
            if !u.view.mods.is_empty() {
                ui.separator();
                worn_mods(ui, &u.view);
            }
            ui.separator();
            ui.label(RichText::new(tr!("AU MARCHÉ")).color(GOLD_DIM).small().strong());
            ui.label(RichText::new(&u.price.0).color(u.price.1).size(16.0).strong());
            if let Some(b) = &u.basis {
                ui.label(RichText::new(b).color(Color32::GRAY).small());
            }
        }
    });
}

/// En-tête d'objet, le même partout (diagnostic, détail d'une case) : l'image à la hauteur du
/// texte (entre 64 et 96 px), puis le titre et sa précision en gris, et des lignes en petit
/// (défenses, implicites…). Renvoie la zone de l'en-tête.
/// `badge` : place gardée à droite pour un badge dessiné à part (verdict du diagnostic).
pub fn item_header(ui: &mut egui::Ui, id: &str, image: Option<&TextureHandle>, title: &str, color: Color32, after: Option<&str>, lines: Vec<egui::WidgetText>, badge: Option<(&str, Color32)>) -> Rect {
    item_header_scaled(ui, id, image, title, color, after, lines, badge, 1.0, false)
}

/// `item_header` agrandi d'un facteur `scale` (image, textes ; les lignes sont à construire à la
/// même échelle, voir `header_lines_scaled`).
#[allow(clippy::too_many_arguments)]
/// `tall_image` : l'image prend toute la hauteur du texte (en-tête agrandi du diagnostic).
pub fn item_header_scaled(ui: &mut egui::Ui, id: &str, image: Option<&TextureHandle>, title: &str, color: Color32, after: Option<&str>, lines: Vec<egui::WidgetText>, badge: Option<(&str, Color32)>, scale: f32, tall_image: bool) -> Rect {
    // Noms du jeu (type de base, unique) dans la langue de l'interface.
    let title = gametext::name(title);
    let after = after.map(gametext::name);
    let (title, after) = (title.as_ref(), after.as_deref());
    let text_h_id = ui.id().with(("item_header", id));
    let text_h: f32 = ui.data(|d| d.get_temp(text_h_id)).unwrap_or(80.0 * scale);
    let img_h = if tall_image { text_h.max(64.0 * scale) } else { text_h.clamp(64.0 * scale, 96.0 * scale) };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0 * scale;
        if let Some(tex) = image {
            let size = tex.size_vec2();
            let max_w = if tall_image { 2.0 * 80.0 * scale } else { 80.0 * scale };
            let k = (img_h / size.y).min(max_w / size.x).min(if tall_image { 3.0 * scale } else { 1.5 * scale });
            ui.image((tex.id(), size * k));
        }
        let text = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0 * scale;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0 * scale;
                ui.label(RichText::new(title).color(color).size(17.0 * scale).strong());
                if let Some(a) = after {
                    ui.label(RichText::new(a).color(Color32::GRAY).size(13.5 * scale));
                }
            });
            for l in lines {
                ui.label(l);
            }
        });
        let h = text.response.rect.height();
        if (h - text_h).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(text_h_id, h));
            ui.ctx().request_repaint();
        }
        if let Some((text, color)) = badge {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| ui.scope_builder(egui::UiBuilder::new().invisible(), |ui| self::badge(ui, text, color)));
        }
    })
    .response
    .rect
}

/// Taille des lignes sous le titre d'un en-tête d'objet (propriétés, implicites).
pub const HEADER_LINE_SIZE: f32 = 13.0;

/// Ligne de propriétés en morceaux : texte, et vrai pour une valeur changée par les mods.
pub type Parts = Vec<(String, bool)>;

/// Colonne « actuellement équipé » d'une pièce, prête à afficher (détail et survol de la case).
pub struct WornView {
    pub image: Option<TextureHandle>,
    pub title: String,
    pub after: Option<String>,
    pub color: Color32,
    /// Arme : ses lignes de dégâts, coups critiques et attaques par seconde, colorées comme en jeu.
    pub weapon: Vec<Vec<(String, Color32)>>,
    /// Défenses, propriétés de fiole, niveau requis.
    pub props: Vec<Parts>,
    pub implicits: Vec<String>,
    /// Mods comparés au build : marque, couleur, texte.
    pub mods: Vec<(&'static str, Color32, String)>,
    pub runes: Vec<RuneLine>,
    pub empty_sockets: usize,
    /// Correspondance au build, en pourcentage (mods visés), s'il y a de quoi la calculer.
    pub score: Option<u32>,
    /// Augments conseillés pour les châsses vides, quand le build n'en recommande pas ici.
    pub suggestions: Vec<RuneLine>,
}

/// Trait gris au-dessus des runes (à la place d'un titre), sur la largeur donnée.
pub fn rune_divider(ui: &mut egui::Ui, width: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 7.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0_f32, Color32::from_gray(60)));
}

/// Runes posées (sous un trait gris), puis châsses vides et ce qui irait dedans.
pub fn worn_runes(ui: &mut egui::Ui, w: &WornView, width: f32) {
    if !w.runes.is_empty() || w.empty_sockets > 0 {
        rune_divider(ui, width);
    }
    if !w.runes.is_empty() {
        rune_lines(ui, &w.runes);
    }
    if w.empty_sockets > 0 {
        ui.label(RichText::new(trn!(w.empty_sockets, "{n} châsse vide", "{n} châsses vides")).color(EMPTY_SOCKET).strong());
        if !w.suggestions.is_empty() {
            ui.label(RichText::new(tr!("Pour ton build :")).color(Color32::GRAY).small());
            rune_lines(ui, &w.suggestions);
        }
    }
}


/// Lignes de l'en-tête (propriétés, implicites, chacune avec sa marque) à l'échelle `scale`.
pub fn header_lines_scaled(props: &[Parts], marks: &[Option<(&str, Color32)>], implicits: &[String], implicit_marks: &[Option<(&str, Color32)>], scale: f32) -> Vec<egui::WidgetText> {
    header_lines_full(&[], props, marks, implicits, implicit_marks, scale)
}

/// Lignes de l'en-tête, avec en tête des lignes déjà colorées (dégâts d'une arme, comme en jeu).
pub fn header_lines_full(colored: &[Vec<(String, Color32)>], props: &[Parts], marks: &[Option<(&str, Color32)>], implicits: &[String], implicit_marks: &[Option<(&str, Color32)>], scale: f32) -> Vec<egui::WidgetText> {
    let font = FontId::proportional(HEADER_LINE_SIZE * scale);
    let colored = colored.iter().map(|line| {
        let mut job = egui::text::LayoutJob::default();
        for (text, color) in line {
            job.append(text, 0.0, egui::TextFormat::simple(font.clone(), *color));
        }
        egui::WidgetText::from(job)
    });
    let with_mark = |job: egui::text::LayoutJob, mark: Option<&Option<(&str, Color32)>>| -> egui::WidgetText {
        let Some(Some((m, c))) = mark else { return job.into() };
        let mut marked = egui::text::LayoutJob::default();
        marked.append(&format!("{m} "), 0.0, egui::TextFormat::simple(font.clone(), *c));
        for s in &job.sections {
            marked.append(&job.text[s.byte_range.clone()], 0.0, s.format.clone());
        }
        marked.into()
    };
    let implicit_job = |l: &str| {
        let mut job = egui::text::LayoutJob::default();
        job.append(&gametext::implicit(l), 0.0, egui::TextFormat::simple(font.clone(), MOD_COLOR));
        job
    };
    colored
        .chain(props.iter().enumerate().map(|(i, p)| with_mark(prop_line_sized(p, HEADER_LINE_SIZE * scale), marks.get(i))))
        .chain(implicits.iter().enumerate().map(|(i, l)| with_mark(implicit_job(l), implicit_marks.get(i))))
        .collect()
}

/// En-tête de la pièce équipée.
pub fn worn_header(ui: &mut egui::Ui, id: &str, w: &WornView) -> Rect {
    worn_header_scaled(ui, id, w, 1.0)
}

/// `worn_header` agrandi d'un facteur `scale`.
pub fn worn_header_scaled(ui: &mut egui::Ui, id: &str, w: &WornView, scale: f32) -> Rect {
    let lines = header_lines_full(&w.weapon, &w.props, &[], &w.implicits, &[], scale);
    item_header_scaled(ui, id, w.image.as_ref(), &w.title, w.color, w.after.as_deref(), lines, None, scale, false)
}

/// Agrandissement du texte courant d'une zone (1 par défaut) : la taille du corps de texte
/// face à celle d'egui.
pub fn body_scale(ui: &egui::Ui) -> f32 {
    egui::TextStyle::Body.resolve(ui.style()).size / 12.5
}

/// Mods de la pièce équipée (blanc : visés par le build, gris : non), précédés d'une marque s'il y en a.
pub fn worn_mods(ui: &mut egui::Ui, w: &WornView) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    // Lignes coupées en deux par le jeu : recollées si c'est ainsi qu'elles se traduisent.
    let texts: Vec<String> = w.mods.iter().map(|(_, _, l)| l.clone()).collect();
    for (i, line) in gametext::lines(&texts) {
        let (mark, color, _) = &w.mods[i];
        let mut job = egui::text::LayoutJob::default();
        if !mark.is_empty() {
            job.append(&format!("{mark}  "), 0.0, egui::TextFormat::simple(font.clone(), *color));
        }
        job.append(&line, 0.0, egui::TextFormat::simple(font.clone(), *color));
        ui.label(job);
    }
}

/// Bandeau d'appel au clic, en bas d'une infobulle : souris (bouton gauche allumé) et action,
/// et à droite le score de correspondance au build s'il y en a un.
pub fn click_hint(ui: &mut egui::Ui, text: &str, score: Option<u32>) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);
    click_band(ui, text, score.map(|s| (tr!("score"), crate::i18n::percent(s), score_color(s))), true, false);
}
/// Une puce d'un bandeau à puces : le côté de l'emplacement, sa valeur (écart, score), et si
/// elle s'ouvre.
pub struct Chip {
    pub label: String,
    pub value: Option<(String, Color32)>,
    pub enabled: bool,
}

/// Bandeau comme `click_band`, avec à droite une puce cliquable par emplacement (anneaux,
/// charmes). Renvoie la puce cliquée.
pub fn chip_band(ui: &mut egui::Ui, text: &str, chips: &[Chip]) -> Option<usize> {
    let any = chips.iter().any(|c| c.enabled);
    let color = if any { GOLD } else { Color32::from_gray(90) };
    let mut clicked = None;
    egui::Frame::new()
        .fill(if any { GOLD_DIM.gamma_multiply(0.22) } else { Color32::from_gray(25) })
        .stroke(Stroke::new(1.0_f32, if any { GOLD_DIM } else { Color32::from_gray(60) }))
        .corner_radius(4)
        .inner_margin(egui::Margin { left: 8, right: 4, top: 4, bottom: 4 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(text).color(color).strong()).selectable(false));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // De droite à gauche : la dernière puce d'abord.
                    for (i, c) in chips.iter().enumerate().rev() {
                        let id = ui.id().with(("chip", text, i));
                        let hovered = c.enabled && ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
                        let fill = if hovered { GOLD_DIM.gamma_multiply(0.55) } else if c.enabled { GOLD_DIM.gamma_multiply(0.3) } else { Color32::from_gray(28) };
                        let shown = egui::Frame::new()
                            .fill(fill)
                            .stroke(Stroke::new(1.0_f32, if c.enabled { GOLD_DIM } else { Color32::from_gray(55) }))
                            .corner_radius(3)
                            .inner_margin(egui::Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                // Même largeur d'un bandeau à l'autre : les puces s'alignent en colonnes.
                                ui.set_min_width(84.0);
                                // Le bandeau se remplit par la droite : la valeur d'abord, pour lire « côté valeur ».
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    let label = if c.enabled { Color32::from_gray(200) } else { Color32::from_gray(90) };
                                    if let Some((v, vc)) = &c.value {
                                        ui.add(egui::Label::new(RichText::new(v).color(*vc).strong()).selectable(false));
                                    }
                                    ui.add(egui::Label::new(RichText::new(&c.label).color(label)).selectable(false));
                                });
                            });
                        let resp = ui.interact(shown.response.rect, id.with("hit"), if c.enabled { Sense::click() } else { Sense::hover() });
                        if c.enabled && resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.hovered() != hovered && c.enabled {
                            ui.ctx().data_mut(|d| d.insert_temp(id, resp.hovered()));
                            ui.ctx().request_repaint();
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        });
    clicked
}


/// Bandeau d'action (souris, texte, score à droite) : celui du bas des infobulles, et cliquable
/// (`sense`) dans le diagnostic. Grisé quand `enabled` est faux.
/// `score` : libellé, valeur affichée et sa couleur, à droite.
pub fn click_band(ui: &mut egui::Ui, text: &str, score: Option<(&str, String, Color32)>, enabled: bool, sense: bool) -> egui::Response {
    let id = ui.id().with(("click_band", text));
    let hovered = sense && enabled && ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    let fill = if hovered { GOLD_DIM.gamma_multiply(0.38) } else { GOLD_DIM.gamma_multiply(0.22) };
    let color = if enabled { GOLD } else { Color32::from_gray(90) };
    let shown = egui::Frame::new()
        .fill(if enabled { fill } else { Color32::from_gray(25) })
        .stroke(Stroke::new(1.0_f32, if enabled { GOLD_DIM } else { Color32::from_gray(60) }))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // Souris dessinée : corps arrondi, bouton gauche plein, raie entre les boutons.
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), Sense::hover());
                let body = r.shrink2(egui::vec2(0.5, 0.5));
                let painter = ui.painter();
                let left = Rect::from_min_max(body.min, egui::pos2(body.center().x, body.min.y + body.height() * 0.45));
                painter.rect_filled(left, CornerRadius { nw: 5, ne: 0, sw: 0, se: 0 }, color);
                painter.rect_stroke(body, CornerRadius::same(5), Stroke::new(1.3_f32, color), StrokeKind::Inside);
                painter.line_segment([egui::pos2(body.min.x, left.max.y), egui::pos2(body.max.x, left.max.y)], Stroke::new(1.0_f32, color));
                painter.line_segment([egui::pos2(body.center().x, body.min.y), egui::pos2(body.center().x, left.max.y)], Stroke::new(1.0_f32, color));
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(text).color(color).strong()).selectable(false));
                if let Some((label, value, value_color)) = score {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(value).color(value_color).size(16.0).strong()).selectable(false));
                        ui.add(egui::Label::new(RichText::new(label).color(Color32::GRAY).small()).selectable(false));
                    });
                }
            });
        });
    if !sense {
        return shown.response;
    }
    let resp = ui.interact(shown.response.rect, id.with("hit"), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let now = resp.hovered();
    if now != hovered {
        ui.ctx().data_mut(|d| d.insert_temp(id, now));
        ui.ctx().request_repaint();
    }
    resp
}

/// Colonne entière de la pièce équipée (infobulle de la case) : en-tête, mods, runes. Sans titre :
/// au survol de la case, c'est forcément la pièce équipée.
/// Couleur d'un score de correspondance au build : vert, orange, rouge.
pub fn score_color(score: u32) -> Color32 {
    if score >= 80 { SOCKET_GOOD } else if score >= 50 { EMPTY_SOCKET } else { SOCKET_WRONG }
}

/// Ce que mesure le score de correspondance (au survol).
fn score_help() -> &'static str {
    tr!("Correspondance au build : mods, implicites et runes visés, chacun à sa part de la valeur visée. Au-delà de 100 % une fois tout atteint.")
}

/// Bloc « correspondance » du détail, en bas de la colonne équipée (en face du marché).
pub fn score_block(ui: &mut egui::Ui, score: u32) {
    ui.label(RichText::new(tr!("CORRESPONDANCE AU BUILD")).color(GOLD_DIM).small().strong()).on_hover_text(score_help());
    ui.label(RichText::new(crate::i18n::percent(score)).color(score_color(score)).size(16.0 * body_scale(ui)).strong()).on_hover_text(score_help());
    ui.label(RichText::new(tr!("mods, implicites et runes visés")).color(Color32::GRAY).small());
}

/// Largeur naturelle de la colonne d'une pièce équipée : sa ligne la plus longue (en-tête avec
/// l'image, mods, runes), pour ajuster l'infobulle et le détail à leur contenu.
pub fn worn_width(ui: &egui::Ui, w: &WornView) -> f32 {
    let text_w = |t: &str, size: f32| ui.fonts(|f| f.layout_no_wrap(t.to_string(), FontId::proportional(size), Color32::WHITE).size().x);
    let small = HEADER_LINE_SIZE;
    let joined = |p: &Parts| p.iter().map(|(t, _)| t.as_str()).collect::<String>();
    let title = text_w(&gametext::name(&w.title), 17.0) + w.after.as_deref().map_or(0.0, |a| 8.0 + text_w(&gametext::name(a), 13.5));
    let header = w.props.iter().map(|p| text_w(&joined(p), small)).chain(w.implicits.iter().map(|l| text_w(&gametext::implicit(l), small))).fold(title, f32::max);
    // Image (au plus 80 px) et son écart avec le texte.
    let header = header + w.image.as_ref().map_or(0.0, |_| 90.0);
    let mods = w.mods.iter().map(|(m, _, l)| text_w(&format!("{m}  {}", gametext::line(l)), 14.0)).fold(0.0_f32, f32::max);
    let runes = w.runes.iter().chain(&w.suggestions).map(|r| 30.0 + text_w(&format!("{}{}", gametext::name(&r.name), r.rest), 14.0)).fold(0.0_f32, f32::max);
    header.max(mods).max(runes) + 8.0
}

pub fn worn_column(ui: &mut egui::Ui, w: &WornView) {
    worn_header(ui, "hover_worn", w);
    if !w.mods.is_empty() {
        ui.separator();
        worn_mods(ui, w);
    }
    worn_runes(ui, w, ui.available_width());
}

/// Ligne de propriétés en petit (« Armure 123 », « Récupère 468 mana en 3 s ») : texte gris,
/// valeurs changées par les mods de l'objet en bleu, comme en jeu.
/// Marque invisible en tête d'un morceau de ligne de propriété : il s'affiche en rouge.
const TOO_HIGH: &str = "\u{2060}";

/// « Niveau requis 62 », le chiffre en blanc, ou en rouge s'il dépasse le niveau du personnage.
pub fn req_level_parts(level: u32, character: u32) -> Parts {
    let n = if level > character { format!("{TOO_HIGH}{level}") } else { level.to_string() };
    vec![(tr!("Niveau requis ").into(), false), (n, false)]
}

pub fn prop_line_sized(parts: &[(String, bool)], size: f32) -> egui::text::LayoutJob {
    let font = FontId::proportional(size);
    let mut job = egui::text::LayoutJob::default();
    for (text, augmented) in parts {
        // Morceau marqué « trop haut » (voir `req_level_parts`) : en rouge, sans sa marque.
        let (text, color) = match text.strip_prefix(TOO_HIGH) {
            Some(t) => (t, SOCKET_WRONG),
            None => (text.as_str(), if *augmented { MOD_COLOR } else { Color32::GRAY }),
        };
        job.append(text, 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    job
}

/// Titre d'un objet et sa précision en gris : un unique se nomme d'abord, les autres par leur
/// type de base (leur nom est tiré au hasard).
pub fn item_title(it: &Item) -> (&str, Option<&str>) {
    if it.rarity == Rarity::Unique || it.base == it.name {
        (it.name.as_str(), (it.base != it.name).then_some(it.base.as_str()))
    } else {
        (it.base.as_str(), Some(it.name.as_str()))
    }
}

pub fn verdict_style(v: crate::compare::Verdict) -> (&'static str, Color32) {
    use crate::compare::Verdict;
    match v {
        Verdict::Better => (tr!("▲ MIEUX"), Color32::from_rgb(90, 200, 90)),
        Verdict::Worse => (tr!("▼ MOINS BIEN"), Color32::from_rgb(225, 80, 70)),
        Verdict::Equal => (tr!("= ÉQUIVALENT"), Color32::from_rgb(170, 170, 170)),
        Verdict::Unknown => (tr!("? À VOIR"), Color32::from_rgb(225, 170, 60)),
        Verdict::Same => (tr!("● ÉQUIPÉ"), Color32::from_rgb(100, 150, 230)),
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

/// Bouton d'action à droite d'un titre de section, à la taille des boutons de la barre du bas
/// (l'en-tête a toujours sa hauteur, avec ou sans bouton : les filets restent alignés).
pub fn header_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let resp = ui
        .scope(|ui| {
            gold_widgets(ui);
            ui.add(egui::Button::new(RichText::new(text).color(GOLD)))
        })
        .inner;
    if ui.rect_contains_pointer(resp.rect) {
        ui.painter().rect_stroke(resp.rect, CornerRadius::same(3), Stroke::new(1.0_f32, GOLD), StrokeKind::Inside);
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
/// Hauteur d'une ligne de titre de section : celle d'un bouton ou d'une liste de taille normale.
fn header_height(ui: &egui::Ui) -> f32 {
    (ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y).max(ui.spacing().interact_size.y)
}

pub fn section_header(ui: &mut egui::Ui, title: &str, count: Option<usize>, actions: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.set_min_height(header_height(ui));
        ui.label(RichText::new(title).color(GOLD).size(15.5).strong());
        if let Some(n) = count {
            ui.label(RichText::new(n.to_string()).color(GOLD_DIM).size(14.0));
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
        ui.set_min_height(header_height(ui));
        ui.label(RichText::new(title).color(GOLD).size(15.5).strong());
        ui.add(egui::Label::new(RichText::new(subtitle).color(Color32::GRAY).size(13.0)).truncate()).on_hover_text(subtitle);
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

/// Courbe d'un cours (un point par jour, en exalts) dans `rect` : prix de gauche à droite,
/// quelques repères de prix à gauche, les dates dessous, et au survol le point du jour.
/// `fmt` met un montant en exalts dans sa devise.
pub fn price_chart(ui: &mut egui::Ui, rect: Rect, points: &[(String, f64)], color: Color32, fmt: &dyn Fn(f64) -> String) {
    let painter = ui.painter_at(rect.expand(2.0));
    if points.len() < 2 {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, tr!("Pas assez de points pour une courbe"), egui::FontId::proportional(13.0), Color32::GRAY);
        return;
    }
    let small = egui::FontId::proportional(11.5);
    let (lo, hi) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, v)| (lo.min(*v), hi.max(*v)));
    let pad = ((hi - lo) * 0.1).max(hi.abs() * 0.02).max(1e-9);
    let (lo, hi) = ((lo - pad).max(0.0), hi + pad);
    // Repères de prix à gauche, dates dessous.
    let gutter = [lo, (lo + hi) / 2.0, hi].iter().map(|v| ui.fonts(|f| f.layout_no_wrap(fmt(*v), small.clone(), Color32::GRAY).size().x)).fold(0.0_f32, f32::max) + 8.0;
    let plot = Rect::from_min_max(egui::pos2(rect.left() + gutter, rect.top() + 6.0), egui::pos2(rect.right() - 4.0, rect.bottom() - 18.0));
    let x = |i: usize| plot.left() + plot.width() * i as f32 / (points.len() - 1) as f32;
    let y = |v: f64| plot.bottom() - plot.height() * ((v - lo) / (hi - lo)) as f32;
    let grid = Stroke::new(1.0_f32, Color32::from_gray(42));
    for v in [lo, (lo + hi) / 2.0, hi] {
        painter.hline(plot.x_range(), y(v), grid);
        painter.text(egui::pos2(plot.left() - 6.0, y(v)), egui::Align2::RIGHT_CENTER, fmt(v), small.clone(), Color32::GRAY);
    }
    // « 2026-10-30 » -> « 30/10 » (« 10/30 » en anglais).
    let day = |d: &str| match (d.get(8..10), d.get(5..7)) {
        (Some(dd), Some(mm)) if crate::i18n::english() => format!("{mm}/{dd}"),
        (Some(dd), Some(mm)) => format!("{dd}/{mm}"),
        _ => d.to_string(),
    };
    let last = points.len() - 1;
    for (i, align) in [(0, egui::Align2::LEFT_TOP), (last / 2, egui::Align2::CENTER_TOP), (last, egui::Align2::RIGHT_TOP)] {
        painter.text(egui::pos2(x(i), plot.bottom() + 4.0), align, day(&points[i].0), small.clone(), Color32::GRAY);
    }
    // Aire sous la courbe : une bande verticale par segment.
    let mut mesh = egui::Mesh::default();
    let fill = color.gamma_multiply(0.18);
    for (i, w) in points.windows(2).enumerate() {
        let base = mesh.vertices.len() as u32;
        let (x0, x1) = (x(i), x(i + 1));
        mesh.colored_vertex(egui::pos2(x0, y(w[0].1)), fill);
        mesh.colored_vertex(egui::pos2(x1, y(w[1].1)), fill);
        mesh.colored_vertex(egui::pos2(x1, plot.bottom()), Color32::TRANSPARENT);
        mesh.colored_vertex(egui::pos2(x0, plot.bottom()), Color32::TRANSPARENT);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    let line: Vec<Pos2> = points.iter().enumerate().map(|(i, (_, v))| egui::pos2(x(i), y(*v))).collect();
    painter.add(egui::Shape::line(line, Stroke::new(2.0_f32, color)));
    // Survol : le jour le plus proche, son prix.
    let resp = ui.interact(plot, ui.id().with("price_chart"), Sense::hover());
    if let Some(p) = resp.hover_pos() {
        let i = (((p.x - plot.left()) / plot.width()) * last as f32).round().clamp(0.0, last as f32) as usize;
        let at = egui::pos2(x(i), y(points[i].1));
        painter.vline(at.x, plot.y_range(), Stroke::new(1.0_f32, Color32::from_gray(90)));
        painter.circle_filled(at, 4.0, color);
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(day(&points[i].0)).color(Color32::GRAY));
            ui.label(RichText::new(fmt(points[i].1)).color(Color32::from_rgb(225, 220, 210)).strong());
        });
    }
}

/// Icône des boutons posés sur une vue (arbre de talents, Atlas).
#[derive(Clone, Copy, PartialEq)]
pub enum ViewIcon {
    /// Plein écran : quatre coins tournés vers l'extérieur.
    Enter,
    /// Sortie du plein écran : quatre coins tournés vers l'intérieur.
    Exit,
    /// Recentrer : une mire.
    Recenter,
}

/// Bouton carré dessiné à la main, sur un fond sombre pour rester lisible par-dessus un arbre.
pub fn view_button(ui: &mut egui::Ui, rect: egui::Rect, icon: ViewIcon) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(("view_button", icon as u8)), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if resp.hovered() { GOLD } else { GOLD_DIM };
    let painter = ui.painter();
    painter.rect(rect, 4.0, Color32::from_rgba_unmultiplied(12, 11, 10, 220), Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
    let inner = rect.shrink(8.0);
    let stroke = Stroke::new(2.0_f32, color);
    if icon == ViewIcon::Recenter {
        let (c, r) = (inner.center(), inner.width() * 0.3);
        painter.circle_stroke(c, r, stroke);
        painter.circle_filled(c, 1.5, color);
        for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
            painter.line_segment([c + d * (r - 2.0), c + d * (inner.width() * 0.5)], stroke);
        }
        return resp;
    }
    let on = icon == ViewIcon::Exit;
    let arm = inner.width() * if on { 0.3 } else { 0.4 };
    for (corner, dx, dy) in [(inner.left_top(), 1.0, 1.0), (inner.right_top(), -1.0, 1.0), (inner.left_bottom(), 1.0, -1.0), (inner.right_bottom(), -1.0, -1.0)] {
        // Sortie : le coin est posé à l'intérieur, branches tournées vers le bord.
        let (c, sx, sy) = if on { (corner + Vec2::new(dx, dy) * arm, -dx, -dy) } else { (corner, dx, dy) };
        painter.line_segment([c, c + Vec2::new(sx * arm, 0.0)], stroke);
        painter.line_segment([c, c + Vec2::new(0.0, sy * arm)], stroke);
    }
    resp
}

/// Combinaison de touches dessinée comme sur un clavier : « [Ctrl] + [C] ».
/// Montant et sa devise dans un seul champ : une bordure commune, un trait entre les deux ; la
/// devise est une petite liste (`units`). Vrai si l'un des deux a changé.
pub fn amount_with_unit(ui: &mut egui::Ui, id: &str, value: &mut f64, unit: &mut String, units: &[&str], range: std::ops::RangeInclusive<f64>) -> bool {
    let mut changed = false;
    let mut split = 0.0;
    let shown = ui.scope(|ui| {
        // Les deux morceaux sans cadre propre : le cadre est dessiné autour de l'ensemble.
        let v = &mut ui.visuals_mut().widgets;
        for w in [&mut v.inactive, &mut v.hovered, &mut v.active, &mut v.open] {
            w.bg_stroke = Stroke::NONE;
            w.bg_fill = Color32::TRANSPARENT;
            w.weak_bg_fill = Color32::TRANSPARENT;
            w.expansion = 0.0;
        }
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.horizontal(|ui| {
            let speed = if unit == "ex" { 1.0 } else { 0.1 };
            // À la hauteur de la liste voisine : les deux textes sur la même ligne.
            let h = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
            let w = ui.fonts(|f| f.layout_no_wrap("10000.0".into(), egui::TextStyle::Button.resolve(ui.style()), Color32::WHITE).size().x) * 0.6 + 16.0;
            changed |= ui.add_sized([w, h], egui::DragValue::new(value).range(range).speed(speed).max_decimals(1)).changed();
            // La liste prend la même hauteur, son texte centré dedans.
            ui.spacing_mut().interact_size.y = h;
            split = ui.cursor().left();
            ui.add_space(1.0);
            let before = unit.clone();
            egui::ComboBox::from_id_salt(id).width(0.0).selected_text(RichText::new(unit.to_uppercase()).color(GOLD)).show_ui(ui, |ui| {
                // En majuscules : à la hauteur des chiffres du montant.
                for u in units {
                    ui.selectable_value(unit, u.to_string(), u.to_uppercase());
                }
            });
            changed |= *unit != before;
        });
    });
    let rect = shown.response.rect;
    let stroke = Stroke::new(1.0_f32, GOLD_DIM);
    ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
    ui.painter().vline(split + 0.5, rect.y_range(), stroke);
    changed
}

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
