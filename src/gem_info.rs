//! Description des gemmes (sorts et supports) pour les infobulles de l'onglet Sorts, d'après
//! RePoE : description du sort, effets fixes (ceux qui ne dépendent pas du niveau de la gemme).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::art::USER_AGENT;
use crate::augments::clean;

const GEMS: &str = "https://repoe-fork.github.io/poe2/skill_gems.min.json";
const SKILLS: &str = "https://repoe-fork.github.io/poe2/skills.min.json";
const CACHE: &str = "gem_info_v3.json";
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GemInfo {
    pub description: String,
    pub effects: Vec<String>,
    /// Gemme non taillée dont on la tire : « Skill », « Spirit » ou « Support ».
    #[serde(default)]
    pub uncut: String,
    /// Niveau minimum de cette gemme non taillée (0 : inconnu).
    #[serde(default)]
    pub uncut_level: u32,
    /// Gemme de lignée (« Uhtred's Rite »…) : trouvée telle quelle, pas taillée.
    #[serde(default)]
    pub lineage: bool,
}

impl GemInfo {
    /// Ne se taille pas dans une gemme non taillée : lignée, ou support sans niveau de taille.
    pub fn special(&self) -> bool {
        self.lineage || (self.uncut == "Support" && self.uncut_level == 0)
    }
}

/// Nom affiché de la gemme -> description et effets.
fn build(gems: &serde_json::Value, skills: &serde_json::Value) -> HashMap<String, GemInfo> {
    let mut out = HashMap::new();
    for g in gems.as_object().into_iter().flatten().map(|(_, g)| g) {
        let Some(name) = g["base_item"]["display_name"].as_str().filter(|n| !n.is_empty()) else { continue };
        if g["base_item"]["release_state"].as_str() != Some("released") {
            continue;
        }
        let skill = g["grants_skills"].get(0).and_then(|s| s.as_str()).map(|id| &skills[id]);
        let description = g["support_text"]
            .as_str()
            .or_else(|| skill.and_then(|s| s["active_skill"]["description"].as_str()))
            .map(clean)
            .unwrap_or_default();
        let mut effects = Vec::new();
        if let Some(s) = skill {
            for set in s["stat_sets"].as_array().into_iter().flatten().take(1) {
                let texts = set["static"]["stat_text"].as_object();
                // Dans l'ordre d'affichage du jeu quand il est donné.
                let order: Vec<&str> = set["static"]["tooltip_order"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect();
                let mut lines: Vec<(usize, String)> = texts
                    .into_iter()
                    .flatten()
                    .filter_map(|(k, v)| {
                        let pos = order.iter().position(|o| k.split('\n').any(|p| p == *o)).unwrap_or(usize::MAX);
                        Some((pos, v.as_str()?.to_string()))
                    })
                    .collect();
                lines.sort_by_key(|(p, _)| *p);
                effects.extend(lines.into_iter().flat_map(|(_, t)| t.lines().map(clean).collect::<Vec<_>>()).filter(|l| !l.is_empty()));
            }
        }
        // Gemme non taillée : supports à part ; sorts permanents (auras, heralds, méta-gemmes)
        // tirés d'une Uncut Spirit Gem ; les autres d'une Uncut Skill Gem.
        let tags: Vec<&str> = g["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).collect();
        let uncut = match g["gem_type"].as_str() {
            Some("support") => "Support",
            Some("spirit") => "Spirit",
            _ if tags.contains(&"persistent") => "Spirit",
            _ => "Skill",
        };
        let uncut_level = g["crafting_level"].as_u64().unwrap_or(0) as u32;
        if !description.is_empty() || !effects.is_empty() {
            let info = GemInfo { description, effects, uncut: uncut.into(), uncut_level, lineage: g["is_lineage"].as_bool().unwrap_or(false) };
            // Une gemme peut apparaître plusieurs fois : on garde celle qui a un niveau.
            let e = out.entry(name.to_string()).or_insert_with(|| info.clone());
            if e.uncut_level == 0 && info.uncut_level > 0 {
                *e = info;
            }
        }
    }
    out
}

fn download() -> Result<HashMap<String, GemInfo>, Box<dyn std::error::Error>> {
    let get = |url: &str| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(ureq::get(url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(120)).call()?.into_json()?)
    };
    Ok(build(&get(GEMS)?, &get(SKILLS)?))
}

fn fresh(path: &Path) -> bool {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age < MAX_AGE)
}

/// Descriptions partagées avec l'interface, chargées (ou téléchargées) en arrière-plan.
#[derive(Clone)]
pub struct GemInfos {
    data: Arc<Mutex<Option<Arc<HashMap<String, GemInfo>>>>>,
    cache: PathBuf,
}

impl GemInfos {
    pub fn start(cache: PathBuf) -> GemInfos {
        let shared = GemInfos { data: Arc::default(), cache };
        shared.load(false);
        shared
    }

    /// Retélécharge le catalogue sans attendre qu'il soit périmé.
    pub fn refresh(&self) {
        self.load(true);
    }

    fn load(&self, force: bool) {
        let (out, cache) = (self.data.clone(), self.cache.clone());
        std::thread::spawn(move || {
            let path = cache.join(CACHE);
            let cached: Option<HashMap<String, GemInfo>> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
            if let Some(c) = &cached {
                if out.lock().unwrap().is_none() {
                    *out.lock().unwrap() = Some(Arc::new(c.clone()));
                }
            }
            if force || cached.is_none() || !fresh(&path) {
                match download() {
                    Ok(c) => {
                        let _ = std::fs::create_dir_all(&cache);
                        let _ = std::fs::write(&path, serde_json::to_string(&c).unwrap_or_default());
                        *out.lock().unwrap() = Some(Arc::new(c));
                    }
                    Err(e) => eprintln!("catalogue indisponible : {e}"),
                }
            }
        });
    }

    pub fn get(&self) -> Option<Arc<HashMap<String, GemInfo>>> {
        self.data.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Réseau : descriptions réelles d'un sort et d'un support.
    #[test]
    #[ignore]
    fn real_descriptions() {
        let info = download().unwrap();
        eprintln!("{} gemmes", info.len());
        let nova = &info["Ice Nova"];
        eprintln!("{nova:?}");
        assert!(nova.description.contains("Knocking Back"));
        let cc = &info["Considered Casting"];
        eprintln!("{cc:?}");
        assert!(cc.effects.iter().any(|e| e.contains("35% more Damage")));
        assert_eq!((nova.uncut.as_str(), nova.uncut_level), ("Skill", 1));
        assert_eq!((cc.uncut.as_str(), cc.uncut_level), ("Support", 2));
        assert_eq!(info["Cast on Elemental Ailment"].uncut, "Spirit");
        assert_eq!(info["Archmage"].uncut, "Spirit");
        assert!(info["Uhtred's Rite"].lineage && info["Uhtred's Rite"].special());
        assert!(!cc.special());
    }
}
