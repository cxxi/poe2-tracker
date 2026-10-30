//! Habillage de l'écran d'équipement, extrait de l'installation du jeu du joueur.
//!
//! Les images restent la propriété de GGG : elles ne sont pas livrées avec poe2-tracker,
//! on les extrait une fois depuis les archives locales du jeu puis on les garde en cache.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

use crate::bundles;

const DIR: &str = "art/textures/interface/2d/2dart/uiimages/ingame/inventorypanel/4k/";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Background,
    LifeFrame,
    ManaFrame,
    CharmFrame,
    CharmBlocker,
}

impl Part {
    const ALL: [Part; 5] = [Part::Background, Part::LifeFrame, Part::ManaFrame, Part::CharmFrame, Part::CharmBlocker];

    fn file(self) -> &'static str {
        match self {
            Part::Background => "inventorypanelupperbackground",
            Part::LifeFrame => "inventorypanelpotionlifeleft",
            Part::ManaFrame => "inventorypanelpotionmanaright",
            Part::CharmFrame => "inventorypanelcharmslots3x1",
            Part::CharmBlocker => "inventorypanelcharmblocker",
        }
    }
}

fn load_png(path: &Path) -> Option<ColorImage> {
    let img = image::open(path).ok()?.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

/// Extrait les images manquantes du jeu vers le cache, puis les charge.
fn prepare(cache: &Path, game_dir: &Path) -> Result<Vec<(Part, ColorImage)>, String> {
    let dir = cache.join("ui");
    let missing: Vec<Part> = Part::ALL.into_iter().filter(|p| !dir.join(format!("{}.png", p.file())).exists()).collect();
    if !missing.is_empty() {
        let index = bundles::Index::open(&game_dir)
            .map_err(|e| tr!("archives du jeu illisibles ({dir}) : {e}", dir = game_dir.display(), e))?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for part in missing {
            let data = index.read(&format!("{DIR}{}.dds", part.file())).map_err(|e| e.to_string())?;
            let img = bundles::decode_dds(&data).map_err(|e| e.to_string())?;
            img.save(dir.join(format!("{}.png", part.file()))).map_err(|e| e.to_string())?;
        }
    }
    Part::ALL
        .into_iter()
        .map(|p| load_png(&dir.join(format!("{}.png", p.file()))).map(|img| (p, img)).ok_or(format!("{}.png illisible", p.file())))
        .collect()
}

pub struct UiArt {
    rx: Receiver<Result<Vec<(Part, ColorImage)>, String>>,
    textures: Vec<(Part, TextureHandle)>,
    pub error: Option<String>,
}

impl UiArt {
    pub fn new(ctx: &egui::Context, cache: PathBuf, game_dir: PathBuf) -> UiArt {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(prepare(&cache, &game_dir));
            ctx.request_repaint();
        });
        UiArt { rx, textures: Vec::new(), error: None }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Ok(result) = self.rx.try_recv() {
            match result {
                Ok(images) => {
                    for (part, img) in images {
                        self.textures.push((part, ctx.load_texture(format!("ui-{part:?}"), img, TextureOptions::LINEAR)));
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    pub fn get(&self, part: Part) -> Option<&TextureHandle> {
        self.textures.iter().find(|(p, _)| *p == part).map(|(_, t)| t)
    }
}

#[cfg(test)]
mod extract_tests {
    /// Extraction ponctuelle d'images du panneau d'inventaire (outil de développement) :
    /// `UI_ART_OUT=dossier cargo test extract_inventory_art -- --ignored`.
    #[test]
    #[ignore]
    fn extract_inventory_art() {
        let out = std::path::PathBuf::from(std::env::var("UI_ART_OUT").unwrap());
        let game = crate::paths::detect().expect("dossier du jeu");
        let index = crate::bundles::Index::open(&game).unwrap();
        for name in std::env::var("UI_ART_FILES").unwrap().split(',') {
            match index.read(&format!("{}{name}.dds", super::DIR)).and_then(|d| crate::bundles::decode_dds(&d)) {
                Ok(img) => {
                    img.save(out.join(format!("{name}.png"))).unwrap();
                    println!("{name}: {}x{}", img.width(), img.height());
                }
                Err(e) => println!("{name}: {e}"),
            }
        }
    }
}
