//! Icônes des passifs, extraites à la demande des archives du jeu du joueur puis gardées en
//! cache disque (jamais livrées avec poe2-tracker).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

use crate::bundles;

fn cache_file(cache: &Path, icon: &str) -> PathBuf {
    let name: String = icon.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    cache.join("passive_icons").join(format!("{name}.png"))
}

fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}

pub struct PassiveIcons {
    tx: Sender<String>,
    rx: Receiver<(String, Option<ColorImage>)>,
    textures: HashMap<String, TextureHandle>,
    pending: HashSet<String>,
    missing: HashSet<String>,
}

impl PassiveIcons {
    pub fn new(ctx: &egui::Context, cache: PathBuf, game_dir: PathBuf) -> PassiveIcons {
        let (tx, req_rx) = channel::<String>();
        let (res_tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut index: Option<bundles::Index> = None;
            let mut bundle_cache = Vec::new();
            for icon in req_rx {
                let file = cache_file(&cache, &icon);
                let img = match image::open(&file) {
                    Ok(i) => Some(i.to_rgba8()),
                    Err(_) => {
                        // Ouverture des archives au premier besoin seulement (~2 s).
                        if index.is_none() {
                            index = bundles::Index::open(&game_dir).ok();
                        }
                        index
                            .as_ref()
                            .and_then(|idx| idx.read_cached(&icon.to_lowercase(), &mut bundle_cache).ok())
                            .and_then(|data| bundles::decode_dds(&data).ok())
                            .inspect(|img| {
                                let _ = std::fs::create_dir_all(file.parent().unwrap());
                                let _ = img.save(&file);
                            })
                    }
                };
                // Les grandes images (médaillon central, 4008 px) sont réduites pour l'affichage.
                let img = img.map(|i| if i.width() > 1024 { image::imageops::resize(&i, 1024, 1024 * i.height() / i.width(), image::imageops::FilterType::Triangle) } else { i });
                if res_tx.send((icon, img.as_ref().map(to_color_image))).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        PassiveIcons { tx, rx, textures: HashMap::new(), pending: HashSet::new(), missing: HashSet::new() }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((icon, img)) = self.rx.try_recv() {
            self.pending.remove(&icon);
            match img {
                Some(img) => {
                    self.textures.insert(icon.clone(), ctx.load_texture(&icon, img, TextureOptions::LINEAR));
                }
                None => {
                    self.missing.insert(icon);
                }
            }
        }
    }

    /// Texture de l'icône, ou `None` en attendant son extraction (demandée au passage).
    pub fn get(&mut self, icon: &str) -> Option<&TextureHandle> {
        if icon.is_empty() || self.missing.contains(icon) {
            return None;
        }
        if !self.textures.contains_key(icon) && self.pending.insert(icon.to_string()) {
            let _ = self.tx.send(icon.to_string());
        }
        self.textures.get(icon)
    }
}
