//! Fin de map et boss de la map, lus à l'écran pendant qu'on est dans une map.
//!
//! En haut à droite, le jeu affiche « Map Objectives » puis l'objectif : « Defeat <boss> »
//! tant que le boss est debout, « Map Completed » ensuite. On capture cette colonne toutes les
//! 3 secondes (puis seulement la bande du panneau, une fois trouvé), on y cherche l'en-tête par corrélation avec un modèle (sa hauteur change avec
//! la mini-carte), puis on compare la ligne dessous au modèle « Map Completed ». Panneau
//! introuvable (inventaire ouvert, infobulle, repaire) : on ne conclut rien.
//!
//! Le nom du boss est lu par OCR (modèles d'ocrs, téléchargés une fois) puis ramené au nom de
//! monstre du jeu le plus proche : une lecture trop éloignée de tout nom est écartée, et il en
//! faut deux identiques.
//!
//! Tout est mesuré en 1080p : l'interface est ancrée à droite et suit la hauteur de la fenêtre.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use image::{GrayImage, RgbImage};

use crate::screen::{self, Rect};

/// Colonne capturée, en pixels 1080p : les 620 derniers pixels de large, de y = 100 à 760.
pub const REF_W: u32 = 620;
const REF_Y: u32 = 100;
const REF_H: u32 = 660;
/// Position des modèles dans la colonne (texte aligné à droite) et écart en-tête → objectif.
const HEADER_X: u32 = 460;
const DONE_X: u32 = 470;
const LINE_DY: u32 = 34;
/// Corrélation minimale : en-tête vu (≥ 0,94 sur les captures, ≤ 0,25 sans), objectif
/// terminé (≥ 0,93, ≤ 0,25 sinon).
const HEADER_MIN: f32 = 0.6;
const DONE_MIN: f32 = 0.7;

const TICK: Duration = Duration::from_secs(1);
/// Entre deux captures : chacune fait attendre le rendu du jeu (lecture de l'écran par le GPU),
/// on en fait donc peu. « Map Completed » vu, la suivante vient au bout d'une seconde.
const EVERY: Duration = Duration::from_secs(3);
/// Entre deux lectures du nom du boss.
const OCR_EVERY: Duration = Duration::from_secs(3);
/// Lectures sans nom de boss après lesquelles on abandonne pour cette map (un boss caché par
/// une étiquette d'objet le reste rarement aussi longtemps).
const GIVE_UP: u32 = 8;
/// En-tête déjà trouvé : seule cette bande autour de lui est capturée (en-tête, objectif et
/// ligne lue par l'OCR), de `y - 8` à `y + 64`.
const BAND_ABOVE: u32 = 8;
const BAND_BELOW: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// Panneau des objectifs introuvable : rien à conclure.
    Hidden,
    /// Objectif en cours, en-tête à cette hauteur de la colonne.
    Objective { y: u32 },
    Completed { y: u32 },
}

/// Image en niveaux de gris, en nombres.
struct Gray {
    w: u32,
    h: u32,
    px: Vec<f32>,
}

impl Gray {
    fn new(img: &GrayImage) -> Gray {
        Gray { w: img.width(), h: img.height(), px: img.as_raw().iter().map(|&v| v as f32).collect() }
    }
}

/// Modèle centré (moyenne retirée), avec sa norme.
struct Template {
    w: u32,
    h: u32,
    px: Vec<f32>,
    norm: f32,
}

impl Template {
    fn new(png: &[u8]) -> Template {
        let g = Gray::new(&image::load_from_memory(png).expect("modèle intégré illisible").into_luma8());
        let mean = g.px.iter().sum::<f32>() / g.px.len() as f32;
        let px: Vec<f32> = g.px.iter().map(|v| v - mean).collect();
        let norm = px.iter().map(|v| v * v).sum::<f32>().sqrt();
        Template { w: g.w, h: g.h, px, norm }
    }

    /// Corrélation normalisée du modèle posé en (x, y) de l'image.
    fn ncc(&self, img: &Gray, x: u32, y: u32) -> f32 {
        if x + self.w > img.w || y + self.h > img.h {
            return 0.0;
        }
        let n = (self.w * self.h) as f32;
        let mut sum = 0.0;
        for r in 0..self.h {
            let row = ((y + r) * img.w + x) as usize;
            sum += img.px[row..row + self.w as usize].iter().sum::<f32>();
        }
        let mean = sum / n;
        let (mut dot, mut var) = (0.0, 0.0);
        for r in 0..self.h {
            let row = ((y + r) * img.w + x) as usize;
            let t = (r * self.w) as usize;
            for (a, b) in img.px[row..row + self.w as usize].iter().zip(&self.px[t..t + self.w as usize]) {
                let a = a - mean;
                dot += a * b;
                var += a * a;
            }
        }
        let d = var.sqrt() * self.norm;
        if d > 0.0 { dot / d } else { 0.0 }
    }

    /// Meilleure corrélation sur ces hauteurs, à ±3 pixels de `x` : (score, y).
    fn best(&self, img: &Gray, x: u32, ys: impl Iterator<Item = u32>) -> (f32, u32) {
        let mut out = (-1.0, 0);
        for y in ys {
            for dx in -3i32..=3 {
                let s = self.ncc(img, (x as i32 + dx).max(0) as u32, y);
                if s > out.0 {
                    out = (s, y);
                }
            }
        }
        out
    }
}

pub struct Detector {
    header: Template,
    done: Template,
    /// Hauteur de l'en-tête à la dernière capture : essayée d'abord.
    last_y: Option<u32>,
}

impl Default for Detector {
    fn default() -> Detector {
        Detector { header: Template::new(include_bytes!("../assets/mapwatch/header.png")), done: Template::new(include_bytes!("../assets/mapwatch/completed.png")), last_y: None }
    }
}

impl Detector {
    /// État du panneau des objectifs dans une colonne capturée (pixels 1080p, `REF_W` de large).
    pub fn panel(&mut self, column: &RgbImage) -> Panel {
        let img = Gray::new(&image::DynamicImage::ImageRgb8(column.clone()).into_luma8());
        let near = self.last_y.map(|y| self.header.best(&img, HEADER_X, y.saturating_sub(2)..y + 3)).filter(|(s, _)| *s >= 0.8);
        let (score, y) = near.unwrap_or_else(|| self.header.best(&img, HEADER_X, 0..img.h.saturating_sub(self.header.h)));
        if score < HEADER_MIN {
            self.last_y = None;
            return Panel::Hidden;
        }
        self.last_y = Some(y);
        let (done, _) = self.done.best(&img, DONE_X, (y + LINE_DY).saturating_sub(3)..y + LINE_DY + 4);
        if done > DONE_MIN { Panel::Completed { y } } else { Panel::Objective { y } }
    }
}

/// Ligne de l'objectif prête pour l'OCR : le texte clair (blanc ou gris, peu saturé) en noir sur
/// blanc, agrandi deux fois. Le fond de la map, coloré ou sombre, disparaît.
fn objective_line(column: &RgbImage, y: u32) -> RgbImage {
    let top = y + LINE_DY - 12;
    let h = 28.min(column.height().saturating_sub(top));
    let raw = image::imageops::crop_imm(column, 0, top, column.width(), h).to_image();
    let mut out = RgbImage::new(raw.width(), raw.height());
    for (o, p) in out.pixels_mut().zip(raw.pixels()) {
        let (lo, hi) = (*p.0.iter().min().unwrap(), *p.0.iter().max().unwrap());
        let v = if lo > 130 && hi - lo < 60 { 0 } else { 255 };
        o.0 = [v; 3];
    }
    image::imageops::resize(&out, out.width() * 2, out.height() * 2, image::imageops::FilterType::Triangle)
}

/// Le boss nommé par le texte lu (« + Defeat Vorrik. The Infernal Engineer »), ramené au nom de
/// monstre le plus proche ; rien si aucun n'est assez proche.
pub fn boss_from_text(text: &str, names: &[String]) -> Option<String> {
    static DEFEAT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"D[e?]f[e?][a-z?]*\s+(.+)$").unwrap());
    let read = text.lines().find_map(|l| DEFEAT.captures(l.trim()))?.get(1)?.as_str().trim().to_lowercase();
    if read.len() < 4 {
        return None;
    }
    let (d, best) = names.iter().map(|n| (levenshtein(&read, &n.to_lowercase()), n)).min_by_key(|(d, _)| *d)?;
    (d <= (best.chars().count() / 5).max(2)).then(|| best.clone())
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + usize::from(ca != *cb)));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Noms des monstres du jeu (en anglais) : toutes les chaînes de leur table qui ressemblent à un
/// nom (pas les chemins ni les identifiants).
pub fn monster_names(game_dir: &Path) -> Result<Vec<String>, String> {
    let index = crate::bundles::Index::open(game_dir).map_err(|e| e.to_string())?;
    let raw = index.read("data/balance/monstervarieties.datc64").map_err(|e| e.to_string())?;
    let heap = raw.windows(8).position(|w| w == [0xbb; 8]).ok_or("table des monstres illisible")? + 8;
    let mut names = std::collections::BTreeSet::new();
    let mut cur: Vec<u16> = Vec::new();
    for c in raw[heap..].chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])) {
        if c != 0 {
            cur.push(c);
            continue;
        }
        let s = String::from_utf16_lossy(&cur);
        cur.clear();
        let looks_like_name = s.len() >= 3 && s.len() <= 60 && s.starts_with(|c: char| c.is_ascii_uppercase()) && !s.contains(['/', '_', '{', '<']);
        if looks_like_name {
            names.insert(s);
        }
    }
    Ok(names.into_iter().collect())
}

/// Modèles de l'OCR : téléchargés une fois depuis le serveur de poe2-tracker, vérifiés.
const MODELS: [(&str, &str); 2] = [
    ("text-detection.onnx", "a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe"),
    ("text-recognition.onnx", "86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5"),
];

fn ensure_models(dir: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (name, sha) in MODELS {
        let path = dir.join(name);
        if path.is_file() {
            continue;
        }
        let url = format!("{}/models/{name}", crate::profile::server());
        crate::log_info!("téléchargement du modèle d'OCR {url}");
        let resp = ureq::get(&url).set("User-Agent", crate::art::USER_AGENT).timeout(Duration::from_secs(120)).call().map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader().take(50 << 20), &mut data).map_err(|e| e.to_string())?;
        let actual: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
        if actual != sha {
            return Err(format!("{name} : empreinte inattendue"));
        }
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, &data).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| e.to_string())?;
    }
    Ok(())
}


struct Reader {
    engine: ocrs::OcrEngine,
    names: Vec<String>,
}

impl Reader {
    fn load(models: &Path, game_dir: &Path) -> Result<Reader, String> {
        ensure_models(models)?;
        let model = |name: &str| rten::Model::load_file(models.join(name)).map_err(|e| format!("{name} : {e}"));
        let params = ocrs::OcrEngineParams { detection_model: Some(model(MODELS[0].0)?), recognition_model: Some(model(MODELS[1].0)?), ..Default::default() };
        let engine = ocrs::OcrEngine::new(params).map_err(|e| e.to_string())?;
        let names = monster_names(game_dir)?;
        crate::log_info!("OCR prêt : {} noms de monstres", names.len());
        Ok(Reader { engine, names })
    }

    fn boss(&self, column: &RgbImage, y: u32) -> Option<String> {
        let line = objective_line(column, y);
        let src = ocrs::ImageSource::from_bytes(line.as_raw(), line.dimensions()).ok()?;
        let input = self.engine.prepare_input(src).ok()?;
        let text = self.engine.get_text(&input).ok()?;
        let boss = boss_from_text(&text, &self.names);
        crate::log_info!("objectif lu : {:?} → {boss:?}", text.replace('\n', " / "));
        boss
    }
}

/// Ce que l'app demande au fil de surveillance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Want {
    /// Map où l'on est (code et seed), ou rien : pas de capture.
    pub map: Option<(String, u64)>,
    /// Nom du boss encore inconnu pour cette map : à lire.
    pub boss: bool,
    pub game_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    /// « Map Completed » affiché (deux captures de suite) dans cette map.
    Completed { map: (String, u64) },
    Boss { map: (String, u64), name: String },
}

pub struct Watch {
    want: Arc<Mutex<Want>>,
    rx: Receiver<Seen>,
}

impl Watch {
    pub fn start(models: PathBuf, ctx: egui::Context) -> Watch {
        let want = Arc::new(Mutex::new(Want::default()));
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = want.clone();
        std::thread::Builder::new().name("mapwatch".into()).spawn(move || run(shared, tx, models, ctx)).expect("fil de surveillance des maps");
        Watch { want, rx }
    }

    pub fn want(&self, w: Want) {
        let mut cur = self.want.lock().unwrap();
        if *cur != w {
            *cur = w;
        }
    }

    pub fn poll(&self) -> Vec<Seen> {
        self.rx.try_iter().collect()
    }
}

/// Colonne de droite de la fenêtre du jeu, ramenée en pixels 1080p. Avec la hauteur de
/// l'en-tête, seule la bande autour de lui est capturée (le reste de la colonne reste noir).
fn capture(grab: &mut dyn screen::Grabber, header: Option<u32>) -> Option<RgbImage> {
    let (w, h) = grab.game_size()?;
    if h < 400 {
        return None;
    }
    let (top, rows) = match header {
        Some(y) => {
            let top = y.saturating_sub(BAND_ABOVE);
            (top, (y + BAND_BELOW).min(REF_H) - top)
        }
        None => (0, REF_H),
    };
    let s = h as f32 / 1080.0;
    let cw = ((REF_W as f32 * s).round() as u32).min(w);
    let r = Rect { x: (w - cw) as i32, y: ((REF_Y + top) as f32 * s).round() as i32, w: cw, h: (rows as f32 * s).round() as u32 };
    let img = grab.grab(r)?;
    let img = if h == 1080 { img } else { image::imageops::resize(&img, REF_W, rows, image::imageops::FilterType::Triangle) };
    if rows == REF_H {
        return Some(img);
    }
    let mut column = RgbImage::new(REF_W, REF_H);
    image::imageops::replace(&mut column, &img, 0, top as i64);
    Some(column)
}

fn run(want: Arc<Mutex<Want>>, tx: Sender<Seen>, models: PathBuf, ctx: egui::Context) {
    let mut grabber: Option<Box<dyn screen::Grabber + Send>> = None;
    let mut detector = Detector::default();
    // OCR chargé à la première lecture ; `Err` : impossible, on n'essaie plus.
    let mut reader: Option<Result<Reader, ()>> = None;
    let mut current: Option<(String, u64)> = None;
    let (mut done_frames, mut done_sent, mut boss_sent) = (0, false, false);
    let mut votes: Vec<String> = Vec::new();
    let mut misses = 0;
    let mut last_ocr: Option<Instant> = None;
    let mut last_grab: Option<Instant> = None;
    let mut seen_game = false;
    loop {
        std::thread::sleep(TICK);
        // Lecture de l'écran coupée dans les réglages.
        if Arc::strong_count(&want) == 1 {
            return;
        }
        let w = want.lock().unwrap().clone();
        if w.map != current {
            current = w.map.clone();
            (done_frames, done_sent, boss_sent) = (0, false, false);
            votes.clear();
            misses = 0;
            detector.last_y = None;
        }
        let Some(map) = current.clone() else { continue };
        // Map terminée : plus rien à lire (le boss ne s'affiche plus).
        if done_sent {
            continue;
        }
        let every = if done_frames > 0 { TICK } else { EVERY };
        if last_grab.is_some_and(|t| t.elapsed() < every) {
            continue;
        }
        last_grab = Some(Instant::now());
        if grabber.is_none() {
            grabber = screen::grabber();
            if grabber.is_none() {
                crate::log_warn!("capture d'écran indisponible : fin de map et boss non détectés");
                return;
            }
        }
        let Some(column) = capture(grabber.as_deref_mut().unwrap(), detector.last_y) else { continue };
        if !seen_game {
            seen_game = true;
            crate::log_info!("fenêtre du jeu capturée ({:?})", grabber.as_deref_mut().unwrap().game_size());
        }
        match detector.panel(&column) {
            Panel::Hidden => {}
            Panel::Completed { .. } => {
                done_frames += 1;
                if done_frames >= 2 && !done_sent {
                    done_sent = true;
                    crate::log_info!("map terminée : {}", map.0);
                    let _ = tx.send(Seen::Completed { map: map.clone() });
                    ctx.request_repaint();
                }
            }
            Panel::Objective { y } => {
                done_frames = 0;
                if !w.boss || boss_sent || last_ocr.is_some_and(|t| t.elapsed() < OCR_EVERY) {
                    continue;
                }
                last_ocr = Some(Instant::now());
                let r = reader.get_or_insert_with(|| {
                    Reader::load(&models, &w.game_dir).map_err(|e| crate::log_warn!("lecture du nom des boss impossible : {e}"))
                });
                let Ok(r) = r else { continue };
                let Some(name) = r.boss(&column, y) else {
                    // Objectif qui n'est pas un boss (« Place Explosives… ») : on arrête de le lire.
                    misses += 1;
                    if misses >= GIVE_UP {
                        boss_sent = true;
                        crate::log_info!("pas de boss lisible dans l'objectif de {}", map.0);
                    }
                    continue;
                };
                if votes.contains(&name) {
                    boss_sent = true;
                    crate::log_info!("boss de {} : {name}", map.0);
                    let _ = tx.send(Seen::Boss { map: map.clone(), name });
                    ctx.request_repaint();
                } else {
                    votes.push(name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> RgbImage {
        let path = format!("{}/tests/mapwatch/{name}.webp", env!("CARGO_MANIFEST_DIR"));
        image::open(path).unwrap().into_rgb8()
    }

    /// Captures de la colonne de droite (y = 200 à 400 en 1080p).
    #[test]
    fn objective_panel_states() {
        let mut d = Detector::default();
        assert_eq!(d.panel(&fixture("vorrik")), Panel::Objective { y: 88 });
        assert!(matches!(d.panel(&fixture("morwyn")), Panel::Objective { .. }));
        assert!(matches!(d.panel(&fixture("completed")), Panel::Completed { .. }));
        assert_eq!(d.panel(&fixture("hidden")), Panel::Hidden);
        // Ligne en partie cachée par l'étiquette d'un objet au sol : l'en-tête est vu.
        assert!(matches!(d.panel(&fixture("occluded")), Panel::Objective { .. }));
    }

    /// Écran 1080p avec une capture posée à sa place.
    struct Screen(RgbImage);

    impl screen::Grabber for Screen {
        fn game_size(&mut self) -> Option<(u32, u32)> {
            Some(self.0.dimensions())
        }
        fn grab(&mut self, r: Rect) -> Option<RgbImage> {
            Some(image::imageops::crop_imm(&self.0, r.x as u32, r.y as u32, r.w, r.h).to_image())
        }
    }

    /// En-tête trouvé dans la colonne entière, puis retrouvé dans sa seule bande.
    #[test]
    fn band_capture_keeps_the_panel() {
        let mut screen = RgbImage::new(1920, 1080);
        image::imageops::replace(&mut screen, &fixture("vorrik"), 1300, 200);
        let mut screen = Screen(screen);
        let mut d = Detector::default();
        assert_eq!(d.panel(&capture(&mut screen, None).unwrap()), Panel::Objective { y: 188 });
        let band = capture(&mut screen, d.last_y).unwrap();
        assert_eq!(band.dimensions(), (REF_W, REF_H));
        assert_eq!(d.panel(&band), Panel::Objective { y: 188 });
        // Panneau déplacé (mini-carte) : la bande ne le voit plus, la capture suivante est entière.
        assert_eq!(d.panel(&capture(&mut screen, Some(400)).unwrap()), Panel::Hidden);
        assert_eq!(d.last_y, None);
    }

    #[test]
    fn boss_names_from_ocr_text() {
        let names: Vec<String> = ["Morwyn, the Kinslayer", "Vorrik, The Infernal Engineer", "Mist Raven", "Rotting Hulk"].map(String::from).to_vec();
        assert_eq!(boss_from_text("+ Defeat Morwyn, the Kinslaver", &names).as_deref(), Some("Morwyn, the Kinslayer"));
        assert_eq!(boss_from_text("S\n\"+ Defeat Vorrik. The Infernal Engineer\nT", &names).as_deref(), Some("Vorrik, The Infernal Engineer"));
        assert_eq!(boss_from_text("A\n\"+ D?feai Morwn, the Kinslaver", &names).as_deref(), Some("Morwyn, the Kinslayer"));
        // Nom à moitié caché : trop loin de tout nom.
        assert_eq!(boss_from_text("\" + Defe Kinslaver", &names), None);
        assert_eq!(boss_from_text("+ Map Completed", &names), None);
    }

    #[test]
    fn objective_line_is_black_on_white() {
        let mut d = Detector::default();
        let col = fixture("vorrik");
        let Panel::Objective { y } = d.panel(&col) else { panic!() };
        let line = objective_line(&col, y);
        assert_eq!(line.width(), REF_W * 2);
        let dark = line.pixels().filter(|p| p.0[0] < 128).count();
        assert!(dark > 1000 && dark < line.pixels().len() / 4, "{dark}");
    }

    /// OCR réel : `POE2_OCR_MODELS=<dossier des .onnx> POE2_GAME_DIR=<jeu> cargo test -- --ignored ocr`.
    #[test]
    #[ignore]
    fn ocr_reads_boss_names() {
        let models = PathBuf::from(std::env::var("POE2_OCR_MODELS").unwrap());
        let game = PathBuf::from(std::env::var("POE2_GAME_DIR").unwrap());
        let r = Reader::load(&models, &game).unwrap();
        let mut d = Detector::default();
        for (f, boss) in [("vorrik", Some("Vorrik, The Infernal Engineer")), ("morwyn", Some("Morwyn, the Kinslayer")), ("occluded", None)] {
            let col = fixture(f);
            let Panel::Objective { y } = d.panel(&col) else { panic!("{f}") };
            let t = Instant::now();
            assert_eq!(r.boss(&col, y).as_deref(), boss, "{f}");
            eprintln!("{f} : {:?}", t.elapsed());
        }
    }
}
