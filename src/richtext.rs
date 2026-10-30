//! Texte enrichi des pages de build (format Lexical de Mobalytics) : paragraphes, titres, listes,
//! liens, gras et italique, et pastilles du jeu (gemme, unique…) avec leur icône.

use eframe::egui::{self, Color32, RichText, TextureHandle};
use serde_json::Value;

use crate::ui::GOLD;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Paragraph,
    Heading(u8),
    Bullet,
    Number(usize),
    Quote,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub link: Option<String>,
    /// Pastille du jeu : son icône et son groupe (« activeSkillGems », « weapons »…).
    pub icon: Option<String>,
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: Kind,
    pub spans: Vec<Span>,
}

/// Blocs d'un document Lexical (`{"root": {...}}`).
pub fn parse(doc: &Value) -> Vec<Block> {
    let mut out = Vec::new();
    for node in doc["root"]["children"].as_array().into_iter().flatten() {
        block(node, &mut out);
    }
    out.retain(|b| b.spans.iter().any(|s| !s.text.trim().is_empty() || s.icon.is_some()));
    out
}

fn block(node: &Value, out: &mut Vec<Block>) {
    let kind = match node["type"].as_str() {
        Some("heading") => Kind::Heading(node["tag"].as_str().and_then(|t| t.trim_start_matches('h').parse().ok()).unwrap_or(3)),
        Some("quote") => Kind::Quote,
        Some("list") => {
            let numbered = node["listType"].as_str() == Some("number");
            let mut n = 0;
            for item in node["children"].as_array().into_iter().flatten() {
                // Liste dans la liste : ses éléments à la suite.
                if item["children"].as_array().is_some_and(|c| c.iter().any(|c| c["type"] == "list")) {
                    for c in item["children"].as_array().into_iter().flatten().filter(|c| c["type"] == "list") {
                        block(c, out);
                    }
                    continue;
                }
                n += 1;
                let kind = if numbered { Kind::Number(n) } else { Kind::Bullet };
                inline_block(item, kind, out);
            }
            return;
        }
        _ => Kind::Paragraph,
    };
    inline_block(node, kind, out);
}

/// Un bloc et ses morceaux ; un retour à la ligne ouvre un nouveau bloc du même genre.
fn inline_block(node: &Value, kind: Kind, out: &mut Vec<Block>) {
    let mut spans = Vec::new();
    inline(node, &Span::default(), &mut spans, kind, out);
    out.push(Block { kind, spans });
}

fn inline(node: &Value, style: &Span, spans: &mut Vec<Span>, kind: Kind, out: &mut Vec<Block>) {
    for c in node["children"].as_array().into_iter().flatten() {
        match c["type"].as_str() {
            Some("text") => {
                let f = c["format"].as_u64().unwrap_or(0);
                spans.push(Span {
                    text: c["text"].as_str().unwrap_or_default().to_string(),
                    bold: style.bold || f & 1 != 0,
                    italic: style.italic || f & 2 != 0,
                    strike: style.strike || f & 4 != 0,
                    underline: style.underline || f & 8 != 0,
                    ..style.clone()
                });
            }
            Some("linebreak") => out.push(Block { kind, spans: std::mem::take(spans) }),
            Some("link") => {
                let link = Span { link: c["url"].as_str().map(str::to_string), ..style.clone() };
                inline(c, &link, spans, kind, out);
            }
            Some("static-data-widget") => spans.push(Span {
                text: c["label"].as_str().unwrap_or_default().to_string(),
                icon: c["icon"].as_str().filter(|i| !i.is_empty()).map(str::to_string),
                group: c["groupId"].as_str().map(str::to_string),
                ..style.clone()
            }),
            _ => inline(c, style, spans, kind, out),
        }
    }
}

/// Couleur d'un lien vers un site connu : YouTube en rouge, Twitch en violet.
fn link_color(url: &str) -> Option<Color32> {
    let url = url.to_lowercase();
    if url.contains("youtube.com") || url.contains("youtu.be") {
        Some(Color32::from_rgb(235, 70, 60))
    } else if url.contains("twitch.tv") {
        Some(Color32::from_rgb(160, 110, 255))
    } else {
        None
    }
}

/// Couleur d'une pastille du jeu : gemmes en bleu, uniques en orange, le reste en or.
fn group_color(s: &Span) -> Color32 {
    match (s.group.as_deref().unwrap_or_default(), s.icon.as_deref().unwrap_or_default()) {
        (g, _) if g.contains("Gems") || g.contains("gems") => Color32::from_rgb(110, 170, 240),
        (_, i) if i.contains("/Uniques/") || i.contains("/Unique/") => crate::ui::rarity_color(crate::item::Rarity::Unique),
        _ => GOLD,
    }
}

/// Ce que l'affichage demande à l'app : l'image d'une pastille (chargée à la demande) et sa fiche
/// au survol.
pub trait Resolver {
    fn icon(&mut self, url: &str) -> Option<TextureHandle>;
    fn tip(&mut self, ui: &mut egui::Ui, span: &Span);
}

/// Affiche les blocs, à la taille de texte `body`. `id` distingue deux textes de la même page
/// (leurs pastilles ne doivent pas partager leur survol).
pub fn show(ui: &mut egui::Ui, id: &str, blocks: &[Block], body: f32, resolver: &mut dyn Resolver) {
    // Largeur de la colonne, fixée une fois : une ligne trop longue (un lien, une regex sans
    // espace) ne l'élargit pas, elle se coupe.
    let width = ui.available_width();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 && matches!(b.kind, Kind::Heading(_)) {
            ui.add_space(8.0);
        }
        ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.set_max_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.spacing_mut().item_spacing.x = 0.0;
            let size = match b.kind {
                Kind::Heading(1 | 2) => body * 1.45,
                Kind::Heading(_) => body * 1.2,
                _ => body,
            };
            // Interligne aéré : c'est un texte à lire ; les lignes passées par les pastilles aussi.
            let line = size * 1.55;
            ui.spacing_mut().item_spacing.y = 0.0;
            match b.kind {
                Kind::Bullet => {
                    ui.label(RichText::new("  •  ").color(GOLD).size(size).line_height(Some(line)));
                }
                Kind::Number(n) => {
                    ui.label(RichText::new(format!("  {n}.  ")).color(GOLD).size(size).line_height(Some(line)));
                }
                Kind::Quote => {
                    ui.label(RichText::new("┃ ").color(Color32::GRAY).size(size).line_height(Some(line)));
                }
                _ => {}
            }
            for (k, s) in b.spans.iter().enumerate() {
                let heading = matches!(b.kind, Kind::Heading(_));
                let color = if s.group.is_some() {
                    group_color(s)
                } else if let Some(url) = &s.link {
                    link_color(url).unwrap_or(ui.visuals().hyperlink_color)
                } else if heading {
                    GOLD
                } else {
                    Color32::from_gray(215)
                };
                let text = if s.group.is_some() { crate::gametext::name(&s.text).into_owned() } else { s.text.clone() };
                // Pastille du jeu : icône et nom ensemble, sa fiche au survol.
                if s.group.is_some() {
                    let tex = s.icon.as_deref().and_then(|u| resolver.icon(u));
                    // Pastille d'un seul tenant : à la ligne si elle ne tient pas au bout de celle-ci.
                    let text_w = ui.fonts(|f| f.layout_no_wrap(text.clone(), egui::FontId::proportional(size), Color32::WHITE).size().x);
                    let pill_w = text_w + if tex.is_some() { size * 1.3 + 3.0 } else { 0.0 };
                    if ui.max_rect().right() - ui.cursor().left() < pill_w {
                        ui.end_row();
                    }
                    let pill = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        if let Some(tex) = &tex {
                            let h = size * 1.3;
                            let k = (h / tex.size_vec2().y).min(h / tex.size_vec2().x);
                            ui.add(egui::Image::new((tex.id(), tex.size_vec2() * k)));
                        }
                        ui.add(egui::Label::new(RichText::new(&text).size(size).color(color).line_height(Some(line))).selectable(false));
                    });
                    // Survol sur toute la pastille, icône et nom.
                    ui.interact(pill.response.rect, ui.id().with(("pill", id, i, k)), egui::Sense::hover())
                        .on_hover_cursor(egui::CursorIcon::Help)
                        .on_hover_ui(|ui| resolver.tip(ui, s));
                    // Développement : fiche d'une pastille ouverte d'office (POE2_TRACKER_GUIDE_TIP=nom).
                    if std::env::var("POE2_TRACKER_GUIDE_TIP").is_ok_and(|n| n == s.text) {
                        let pos = pill.response.rect.left_bottom();
                        egui::Area::new(ui.id().with(("pill_tip", id, i, k))).order(egui::Order::Tooltip).fixed_pos(pos).show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| resolver.tip(ui, s));
                        });
                    }
                    continue;
                }
                let mut t = RichText::new(text).size(size).color(color).line_height(Some(line));
                if s.bold || heading {
                    t = t.strong();
                }
                if s.italic {
                    t = t.italics();
                }
                if s.strike {
                    t = t.strikethrough();
                }
                if s.underline {
                    t = t.underline();
                }
                match &s.link {
                    Some(url) => {
                        ui.hyperlink_to(t, url);
                    }
                    None => {
                        ui.label(t);
                    }
                }
            }
        });
        if matches!(b.kind, Kind::Paragraph | Kind::Heading(_) | Kind::Quote) {
            ui.add_space(4.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lexical_blocks() {
        let doc: Value = serde_json::from_str(r#"{"root": {"children": [
            {"type": "heading", "tag": "h2", "children": [{"type": "text", "text": "Overview", "format": 1}]},
            {"type": "paragraph", "children": [
                {"type": "text", "text": "Uses ", "format": 0},
                {"type": "static-data-widget", "label": "Ice Nova", "icon": "https://x/ice.webp", "groupId": "activeSkillGems"},
                {"type": "linebreak"},
                {"type": "link", "url": "https://youtu.be/x", "children": [{"type": "text", "text": "video", "format": 2}]}
            ]},
            {"type": "list", "listType": "bullet", "children": [
                {"type": "listitem", "children": [{"type": "text", "text": "one", "format": 0}]},
                {"type": "listitem", "children": [{"type": "text", "text": "two", "format": 0}]}
            ]},
            {"type": "paragraph", "children": []}
        ]}}"#).unwrap();
        let b = parse(&doc);
        assert_eq!(b.len(), 5);
        assert_eq!(b[0].kind, Kind::Heading(2));
        assert!(b[0].spans[0].bold);
        assert_eq!(b[1].spans[1].icon.as_deref(), Some("https://x/ice.webp"));
        assert_eq!(b[2].spans[0].link.as_deref(), Some("https://youtu.be/x"));
        assert!(b[2].spans[0].italic);
        assert_eq!((b[3].kind, b[4].kind), (Kind::Bullet, Kind::Bullet));
    }
}
