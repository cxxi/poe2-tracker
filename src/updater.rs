//! Mise à jour depuis les versions publiées sur GitHub : vérification au lancement puis toutes
//! les 5 minutes (le bouton apparaît sans relancer), téléchargement et remplacement de l'exécutable sur demande (bouton).
//! Chaque binaire publié a son empreinte SHA-256, vérifiée avant installation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::art::USER_AGENT;

const LATEST: &str = "https://api.github.com/repos/cxxi/poe2-tracker/releases/latest";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Bien sous la limite de l'API GitHub sans compte (60 requêtes par heure).
const EVERY: Duration = Duration::from_secs(5 * 60);

/// Binaire publié pour ce système.
fn asset_name() -> &'static str {
    if cfg!(windows) { "poe2-tracker-windows-x86_64.exe" } else { "poe2-tracker-linux-x86_64" }
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    UpToDate,
    Available { version: String, url: String, sha_url: String },
    Downloading { version: String },
    /// Nouvelle version en place : il reste à relancer.
    Ready { version: String },
    Failed(String),
}

#[derive(Clone)]
pub struct Updater {
    state: Arc<Mutex<State>>,
    /// Recherche automatique activée (réglage).
    enabled: Arc<AtomicBool>,
}

/// "0.10.2" > "0.9.7" : comparaison numérique, partie par partie.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| v.trim_start_matches('v').split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(candidate) > parts(current)
}

fn get(url: &str) -> Result<ureq::Response, String> {
    ureq::get(url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(60)).call().map_err(|e| e.to_string())
}

fn check() -> Result<State, String> {
    check_against(VERSION)
}

fn check_against(current: &str) -> Result<State, String> {
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        assets: Vec<Asset>,
    }
    let r: Release = get(LATEST)?.into_json().map_err(|e| e.to_string())?;
    if !newer(&r.tag_name, current) {
        return Ok(State::UpToDate);
    }
    let find = |name: &str| r.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    let url = find(asset_name()).ok_or("binaire absent de la version publiée")?;
    let sha_url = find(&format!("{}.sha256", asset_name())).ok_or("empreinte absente de la version publiée")?;
    Ok(State::Available { version: r.tag_name.trim_start_matches('v').to_string(), url, sha_url })
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut get(url)?.into_reader(), &mut data).map_err(|e| e.to_string())?;
    Ok(data)
}

/// Binaire téléchargé, seulement si son empreinte correspond à celle publiée.
fn download_verified(url: &str, sha_url: &str) -> Result<Vec<u8>, String> {
    let data = download(url)?;
    let expected = String::from_utf8_lossy(&download(sha_url)?).split_whitespace().next().unwrap_or_default().to_lowercase();
    let actual: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
    if expected != actual {
        return Err("empreinte incorrecte : téléchargement corrompu, rien n'a été remplacé".into());
    }
    Ok(data)
}

/// Télécharge, vérifie l'empreinte et remplace l'exécutable en cours (possible même pendant
/// qu'il tourne, sous Windows comme sous Linux).
fn install(url: &str, sha_url: &str) -> Result<(), String> {
    let data = download_verified(url, sha_url)?;
    let tmp = std::env::temp_dir().join(format!("poe2-tracker-update-{}", std::process::id()));
    std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
    let result = self_replace::self_replace(&tmp).map_err(|e| format!("remplacement impossible : {e}"));
    let _ = std::fs::remove_file(&tmp);
    result
}

impl Updater {
    pub fn start(ctx: &egui::Context, enabled: bool) -> Updater {
        let updater = Updater {
            state: Arc::new(Mutex::new(State::UpToDate)),
            enabled: Arc::new(AtomicBool::new(enabled)),
        };
        if std::env::var_os("POE2_TRACKER_NO_UPDATE").is_some() {
            return updater;
        }
        let (shared, ctx) = (updater.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut last: Option<Instant> = None;
            loop {
                // Une vérification ne remplace pas un téléchargement en cours ou déjà fait ; une
                // version proposée peut l'être par une plus récente.
                let due = last.is_none_or(|t| t.elapsed() >= EVERY);
                if shared.enabled.load(Ordering::Relaxed)
                    && due
                    && matches!(shared.state(), State::UpToDate | State::Failed(_) | State::Available { .. }) {
                    last = Some(Instant::now());
                    if let Ok(s) = check() {
                    *shared.state.lock().unwrap() = s;
                    ctx.request_repaint();
                }
            }
            std::thread::sleep(Duration::from_secs(5));
            }
        });
        updater
    }

    /// Active ou coupe la recherche automatique ; coupée, une version proposée n'est plus montrée.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        if !on && matches!(*state, State::Available { .. } | State::Failed(_)) {
            *state = State::UpToDate;
        }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Lance la mise à jour proposée (bouton).
    pub fn install(&self, ctx: &egui::Context) {
        let State::Available { version, url, sha_url } = self.state() else { return };
        *self.state.lock().unwrap() = State::Downloading { version: version.clone() };
        let (shared, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            let next = match install(&url, &sha_url) {
                Ok(()) => State::Ready { version },
                Err(e) => State::Failed(e),
            };
            *shared.state.lock().unwrap() = next;
            ctx.request_repaint();
        });
    }
}

/// Chemin de l'exécutable, relevé au lancement : une fois le fichier remplacé, Linux ne le
/// donne plus (« poe2-tracker (deleted) »).
static EXE: OnceLock<PathBuf> = OnceLock::new();

/// À appeler au tout début du programme.
pub fn remember_exe() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = EXE.set(exe);
    }
}

/// Relance l'exécutable (la nouvelle version) ; l'appelant ferme ensuite la fenêtre.
pub fn restart() -> Result<(), String> {
    let exe = match EXE.get() {
        Some(p) => p.clone(),
        None => {
            let p = std::env::current_exe().map_err(|e| e.to_string())?;
            PathBuf::from(p.to_string_lossy().trim_end_matches(" (deleted)"))
        }
    };
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Réseau : la dernière version publiée est trouvée, téléchargée et son empreinte vérifiée.
    #[test]
    #[ignore]
    fn published_release_downloads_and_verifies() {
        let State::Available { version, url, sha_url } = check_against("0.0.1").unwrap() else { panic!("aucune version publiée") };
        let data = download_verified(&url, &sha_url).unwrap();
        eprintln!("version {version} : {} octets vérifiés", data.len());
        assert!(data.len() > 1_000_000);
    }

    #[test]
    fn version_order() {
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("0.2.1", "0.2.0"));
        assert!(!newer("v0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
    }
}
