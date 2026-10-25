//! Détection des copies d'objets faites en jeu.
//!
//! - Ctrl+C       : diagnostiquer l'objet survolé.
//! - Ctrl+Alt+C   : l'ajouter à l'équipement suivi (copie avancée native du jeu).
//!
//! On ne capture aucun raccourci : on lit l'état du clavier (lecture passive, seulement
//! Ctrl/Alt/C) au moment où C est enfoncé, puis le texte que le jeu a mis dans le
//! presse-papier. Les autres applications gardent leurs raccourcis.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use device_query::{DeviceQuery, DeviceState, Keycode};
use eframe::egui;

use crate::item;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKind {
    Diagnose,
    Equip,
}

pub struct CopyEvent {
    pub kind: CopyKind,
    pub text: String,
}

const TICK: Duration = Duration::from_millis(10);
/// Délai minimal pour laisser le jeu remplir le presse-papier.
const SETTLE: Duration = Duration::from_millis(80);
/// Au-delà, un texte identique au précédent est accepté (même objet recopié).
const SAME_TEXT_AFTER: Duration = Duration::from_millis(350);
const GIVE_UP: Duration = Duration::from_millis(1000);

pub fn spawn(tx: Sender<CopyEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let keyboard = DeviceState::new();
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("presse-papier inaccessible : {e}");
                return;
            }
        };
        let mut last_text = clipboard.get_text().unwrap_or_default();
        let mut prev_c = false;
        let mut pending: Option<(CopyKind, Instant)> = None;

        loop {
            std::thread::sleep(TICK);
            let keys = keyboard.get_keys();
            let c_down = keys.contains(&Keycode::C);
            let ctrl = keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl);
            let alt = keys.contains(&Keycode::LAlt) || keys.contains(&Keycode::RAlt);
            if c_down && !prev_c && ctrl {
                let kind = if alt { CopyKind::Equip } else { CopyKind::Diagnose };
                pending = Some((kind, Instant::now()));
            }
            prev_c = c_down;

            // Le presse-papier n'est lu qu'après un Ctrl+C : le lire en continu interroge sans
            // cesse l'application qui le possède (Discord plantait en partage d'écran).
            let Some((kind, at)) = pending else { continue };
            let text = clipboard.get_text().unwrap_or_default();

            let age = at.elapsed();
            let event = if age > GIVE_UP {
                pending = None; // Ctrl+C ailleurs que sur un objet
                None
            } else if age >= SETTLE && item::is_item_text(&text) && (text != last_text || age >= SAME_TEXT_AFTER) {
                pending = None;
                Some(kind)
            } else {
                None
            };
            if text != last_text {
                last_text = text.clone();
            }
            if let Some(kind) = event {
                if tx.send(CopyEvent { kind, text }).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    });
}
