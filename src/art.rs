//! Illustrations des objets, téléchargées une fois puis gardées en cache disque.
//!
//! - Types de base (rares, magiques, normaux) : index RePoE « nom -> fichier d'art ».
//! - Uniques : icône fournie par poe.ninja.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

use crate::item::{Item, Rarity};

pub const USER_AGENT: &str = "poe2-tracker/0.1 (outil perso)";
const REPOE_BASES: &str = "https://repoe-fork.github.io/poe2/base_items.json";
const REPOE_ART: &str = "https://repoe-fork.github.io/poe2/";
const NINJA: &str = "https://poe.ninja/poe2/api/economy/stash/current/item/overview";
const UNIQUE_TYPES: &[&str] = &[
    "UniqueWeapons", "UniqueArmours", "UniqueAccessories", "UniqueFlasks", "UniqueJewels",
    "UniqueCharms",
];
const INDEX_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Serialize, Deserialize)]
pub struct BaseArt {
    pub art: String,
    pub w: u8,
    pub h: u8,
}

#[derive(Default, Serialize, Deserialize)]
pub struct ArtIndex {
    pub bases: HashMap<String, BaseArt>,
    /// Nom d'unique -> URL d'icône.
    pub uniques: HashMap<String, String>,
    /// Noms de base, du plus long au plus court, pour retrouver la base d'un objet magique.
    #[serde(skip)]
    by_length: Vec<String>,
}

/// Icône d'unique en pleine résolution : poe.ninja donne celle du site du jeu, réduite
/// (47 px par case d'inventaire, floue une fois agrandie) ; son chemin, encodé dans l'adresse
/// (« …/gen/image/<base64 de [25,14,{"f":"2DItems/…"}]>/… »), sert à prendre celle de RePoE.
fn hd_unique(url: &str) -> String {
    let path = url
        .split("/gen/image/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(base64_url)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| v.as_array()?.iter().find_map(|x| x["f"].as_str().map(str::to_string)));
    match path {
        Some(f) => format!("{REPOE_ART}Art/{f}.png"),
        None => url.to_string(),
    }
}

/// Base64 « URL » (sans remplissage), assez pour les adresses d'images du jeu.
fn base64_url(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

impl ArtIndex {
    fn finish(mut self) -> Self {
        self.by_length = self.bases.keys().cloned().collect();
        self.by_length.sort_by_key(|n| std::cmp::Reverse(n.len()));
        self
    }

    /// Vrai type de base : un objet magique s'appelle "Greater Mana Flask of the Constant".
    pub fn base_of(&self, it: &Item) -> Option<String> {
        if self.bases.contains_key(&it.base) {
            return Some(it.base.clone());
        }
        self.by_length.iter().find(|b| it.name.contains(b.as_str())).cloned()
    }

    pub fn url_for(&self, it: &Item) -> Option<String> {
        if it.rarity == Rarity::Unique {
            if let Some(url) = self.uniques.get(&it.name) {
                return Some(hd_unique(url));
            }
        }
        let base = self.base_of(it)?;
        Some(format!("{REPOE_ART}{}", self.bases[&base].art.replace(".dds", ".png")))
    }
}

fn get(url: &str) -> Result<ureq::Response, Box<dyn std::error::Error>> {
    Ok(ureq::get(url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(60)).call()?)
}

fn fresh(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or_default() < INDEX_MAX_AGE)
        .unwrap_or(false)
}

fn build_index(league: &str) -> Result<ArtIndex, Box<dyn std::error::Error>> {
    #[derive(Deserialize)]
    struct Visual {
        dds_file: String,
    }
    #[derive(Deserialize)]
    struct Base {
        name: String,
        release_state: Option<String>,
        visual_identity: Option<Visual>,
        inventory_width: Option<u8>,
        inventory_height: Option<u8>,
    }
    let raw: HashMap<String, Base> = serde_json::from_reader(get(REPOE_BASES)?.into_reader())?;
    let mut index = ArtIndex::default();
    for b in raw.into_values() {
        if b.release_state.as_deref() != Some("released") || b.name.is_empty() {
            continue;
        }
        if let Some(v) = b.visual_identity {
            index.bases.entry(b.name).or_insert(BaseArt {
                art: v.dds_file,
                w: b.inventory_width.unwrap_or(1),
                h: b.inventory_height.unwrap_or(1),
            });
        }
    }
    #[derive(Deserialize)]
    struct Line {
        name: String,
        icon: Option<String>,
    }
    #[derive(Deserialize)]
    struct Overview {
        lines: Vec<Line>,
    }
    for t in UNIQUE_TYPES {
        let url = format!("{NINJA}?league={}&type={t}", league.replace(' ', "+"));
        // Les icônes d'uniques sont un bonus : un échec ne bloque pas l'index.
        if let Ok(resp) = get(&url) {
            if let Ok(o) = resp.into_json::<Overview>() {
                for l in o.lines {
                    if let Some(icon) = l.icon {
                        index.uniques.entry(l.name).or_insert(icon);
                    }
                }
            }
        }
    }
    Ok(index)
}

/// Charge l'index depuis le cache ou le reconstruit (en arrière-plan). Un index sans icônes
/// d'uniques (poe.ninja ne connaît pas la ligue, ou ne répondait pas) n'est pas gardé : il
/// sera refait au prochain chargement.
fn load_index(cache: &Path, league: &str, force: bool) -> Option<ArtIndex> {
    let path = cache.join("art_index.json");
    if fresh(&path) && !force {
        if let Some(idx) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<ArtIndex>(&s).ok()).filter(|i| !i.uniques.is_empty()) {
            return Some(idx.finish());
        }
    }
    match build_index(league) {
        Ok(idx) => {
            if !idx.uniques.is_empty() {
                let _ = std::fs::create_dir_all(cache);
                let _ = std::fs::write(&path, serde_json::to_string(&idx).unwrap_or_default());
            }
            Some(idx.finish())
        }
        Err(e) => {
            eprintln!("index des illustrations indisponible : {e}");
            // Hors ligne : on reprend l'ancien index même périmé.
            std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<ArtIndex>(&s).ok()).map(ArtIndex::finish)
        }
    }
}

fn file_name(url: &str) -> String {
    url.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect()
}

fn fetch_image(cache: &Path, url: &str) -> Option<ColorImage> {
    let path = cache.join("img").join(file_name(url));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let mut b = Vec::new();
            get(url).ok()?.into_reader().read_to_end(&mut b).ok()?;
            let _ = std::fs::create_dir_all(path.parent()?);
            let _ = std::fs::write(&path, &b);
            b
        }
    };
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

const GRAY: &str = "gray:";

fn grayscale(mut img: ColorImage) -> ColorImage {
    for p in img.pixels.iter_mut() {
        let [r, g, b, a] = p.to_srgba_unmultiplied();
        let l = (0.3 * r as f32 + 0.59 * g as f32 + 0.11 * b as f32) as u8;
        *p = egui::Color32::from_rgba_unmultiplied(l, l, l, a);
    }
    img
}

/// Illustrations prêtes à afficher, chargées à la demande.
pub struct Assets {
    pub index: Arc<Mutex<Option<ArtIndex>>>,
    /// Chargement de l'index en cours le plus récent : un chargement dépassé (autre ligue)
    /// n'écrase pas le suivant.
    generation: Arc<std::sync::atomic::AtomicUsize>,
    league: String,
    cache: PathBuf,
    ctx: egui::Context,
    requests: Sender<String>,
    results: Receiver<(String, Option<ColorImage>)>,
    textures: HashMap<String, TextureHandle>,
    pending: HashSet<String>,
    missing: HashSet<String>,
}

impl Assets {
    pub fn new(ctx: &egui::Context, cache: PathBuf, league: String) -> Assets {
        let index = Arc::new(Mutex::new(None));
        let (req_tx, req_rx) = channel::<String>();
        let (res_tx, res_rx) = channel();
        {
            let (ctx, cache) = (ctx.clone(), cache.clone());
            std::thread::spawn(move || {
                for url in req_rx {
                    // « gray:<url> » : même image, en niveaux de gris.
                    let img = match url.strip_prefix(GRAY) {
                        Some(u) => fetch_image(&cache, u).map(grayscale),
                        None => fetch_image(&cache, &url),
                    };
                    if res_tx.send((url, img)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        let assets = Assets {
            index,
            generation: Arc::default(),
            league,
            cache,
            ctx: ctx.clone(),
            requests: req_tx,
            results: res_rx,
            textures: HashMap::new(),
            pending: HashSet::new(),
            missing: HashSet::new(),
        };
        assets.load_index(false);
        assets
    }

    /// (Re)charge l'index des illustrations pour la ligue en cours, en arrière-plan.
    fn load_index(&self, force: bool) {
        use std::sync::atomic::Ordering;
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (index, generation, cache, league, ctx) = (self.index.clone(), self.generation.clone(), self.cache.clone(), self.league.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let idx = load_index(&cache, &league, force);
            if generation.load(Ordering::SeqCst) == mine && idx.is_some() {
                *index.lock().unwrap() = idx;
                ctx.request_repaint();
            }
        });
    }

    /// La ligue est connue (ou a changé) : l'index est refait s'il lui manque les uniques.
    pub fn set_league(&mut self, league: &str) {
        if league == self.league {
            return;
        }
        self.league = league.to_string();
        let has_uniques = self.index.lock().unwrap().as_ref().is_some_and(|i| !i.uniques.is_empty());
        if !has_uniques {
            self.load_index(false);
        }
    }

    /// Retélécharge l'index (menu « Mettre à jour les catalogues »).
    pub fn refresh(&mut self) {
        self.missing.clear();
        self.load_index(true);
    }

    /// À appeler à chaque image : transforme les téléchargements terminés en textures.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((url, img)) = self.results.try_recv() {
            self.pending.remove(&url);
            match img {
                Some(img) => {
                    let tex = ctx.load_texture(&url, img, TextureOptions::LINEAR);
                    self.textures.insert(url, tex);
                }
                None => {
                    self.missing.insert(url);
                }
            }
        }
    }

    /// Illustration d'un type de base par son nom (gemmes : « Ice Nova », « Zenith I »…).
    pub fn texture_by_name(&mut self, name: &str) -> Option<TextureHandle> {
        let url = {
            let idx = self.index.lock().unwrap();
            let base = idx.as_ref()?.bases.get(name)?;
            format!("{REPOE_ART}{}", base.art.replace(".dds", ".png"))
        };
        self.texture_url(url)
    }

    /// Icône d'un unique par son nom (poe.ninja).
    pub fn texture_unique(&mut self, name: &str) -> Option<TextureHandle> {
        let url = self.index.lock().unwrap().as_ref()?.uniques.get(name)?.clone();
        self.texture_hd(url)
    }

    /// Icône d'un unique en noir et blanc (pièce recommandée sur une case vide).
    pub fn texture_unique_gray(&mut self, name: &str) -> Option<TextureHandle> {
        let url = self.index.lock().unwrap().as_ref()?.uniques.get(name)?.clone();
        let hd = format!("{GRAY}{}", hd_unique(&url));
        if self.missing.contains(&hd) {
            return self.texture_url(format!("{GRAY}{url}"));
        }
        self.texture_url(hd)
    }

    /// Icône d'unique en pleine résolution, ou celle de poe.ninja si RePoE ne l'a pas.
    fn texture_hd(&mut self, url: String) -> Option<TextureHandle> {
        let hd = hd_unique(&url);
        if self.missing.contains(&hd) {
            return self.texture_url(url);
        }
        self.texture_url(hd)
    }

    /// Même illustration qu'à `texture_by_name`, en niveaux de gris.
    pub fn texture_by_name_gray(&mut self, name: &str) -> Option<TextureHandle> {
        let url = {
            let idx = self.index.lock().unwrap();
            let base = idx.as_ref()?.bases.get(name)?;
            format!("{GRAY}{REPOE_ART}{}", base.art.replace(".dds", ".png"))
        };
        self.texture_url(url)
    }

    pub fn texture(&mut self, it: &Item) -> Option<TextureHandle> {
        let unique = (it.rarity == Rarity::Unique).then(|| self.index.lock().unwrap().as_ref()?.uniques.get(&it.name).cloned()).flatten();
        if let Some(url) = unique {
            return self.texture_hd(url);
        }
        let url = self.index.lock().unwrap().as_ref()?.url_for(it)?;
        self.texture_url(url)
    }

    pub fn texture_url(&mut self, url: String) -> Option<TextureHandle> {
        if let Some(t) = self.textures.get(&url) {
            return Some(t.clone());
        }
        if !self.pending.contains(&url) && !self.missing.contains(&url) {
            self.pending.insert(url.clone());
            let _ = self.requests.send(url);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_icons_use_full_resolution_art() {
        let ninja = "https://web.poecdn.com/gen/image/WzI1LDE0LHsiZiI6IjJESXRlbXMvT2ZmaGFuZC9TaGllbGRzL1VuaXF1ZXMvU3ZhbGlubiIsInciOjIsImgiOjQsInNjYWxlIjoxLCJyZWFsbSI6InBvZTIifV0/9725b3d511/Svalinn.png";
        assert_eq!(hd_unique(ninja), format!("{REPOE_ART}Art/2DItems/Offhand/Shields/Uniques/Svalinn.png"));
        assert_eq!(hd_unique("https://example.org/x.png"), "https://example.org/x.png");
    }
    use crate::item::{self, tests::MANA_FLASK};

    #[test]
    fn magic_item_base_is_found_in_its_name() {
        let mut idx = ArtIndex::default();
        for n in ["Mana Flask", "Greater Mana Flask", "Ring"] {
            idx.bases.insert(n.into(), BaseArt { art: format!("Art/{n}.dds"), w: 1, h: 2 });
        }
        let idx = idx.finish();
        let it = item::parse(MANA_FLASK).unwrap();
        assert_eq!(idx.base_of(&it).as_deref(), Some("Greater Mana Flask"));
        assert_eq!(idx.url_for(&it).unwrap(), format!("{REPOE_ART}Art/Greater Mana Flask.png"));
    }
}
