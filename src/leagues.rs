//! Ligues en cours, lues sur le site du jeu (liste publique du site d'échange).
//!
//! Le log du client ne dit pas dans quelle ligue joue un personnage : la ligue se choisit par
//! personnage, avec par défaut la première ligue en cours (ni Standard ni Hardcore).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;

use crate::art::USER_AGENT;

const URL: &str = "https://www.pathofexile.com/api/trade2/data/leagues";

#[derive(Clone, Default)]
pub struct Leagues(Arc<Mutex<Vec<String>>>);

fn download() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    #[derive(Deserialize)]
    struct League {
        id: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        result: Vec<League>,
    }
    let r: Resp = ureq::get(URL).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call()?.into_json()?;
    Ok(r.result.into_iter().map(|l| l.id).collect())
}

impl Leagues {
    /// Liste en cache tout de suite, puis mise à jour en arrière-plan.
    pub fn start(ctx: &egui::Context, cache: PathBuf) -> Leagues {
        let file = cache.join("leagues.json");
        let cached: Vec<String> = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let leagues = Leagues(Arc::new(Mutex::new(cached)));
        let (shared, ctx) = (leagues.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Ok(list) = download() {
                let _ = std::fs::create_dir_all(file.parent().unwrap());
                let _ = std::fs::write(&file, serde_json::to_string(&list).unwrap_or_default());
                *shared.0.lock().unwrap() = list;
                ctx.request_repaint();
            }
        });
        leagues
    }

    pub fn list(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    /// Ligue proposée par défaut : la première en cours, en mode normal.
    pub fn default_league(&self) -> Option<String> {
        self.list().into_iter().find(|l| is_temporary_softcore(l))
    }
}

fn is_temporary_softcore(id: &str) -> bool {
    !matches!(id, "Standard" | "Hardcore") && !id.starts_with("HC ") && !id.contains("SSF") && !id.contains("Hardcore")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_first_temporary_softcore_league() {
        let l = Leagues(Arc::new(Mutex::new(
            ["Standard", "HC Forbidden Rites", "Forbidden Rites", "Runes of Aldur"].map(String::from).to_vec(),
        )));
        assert_eq!(l.default_league().as_deref(), Some("Forbidden Rites"));
    }
}
