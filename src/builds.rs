//! Builds suivis : chacun est un ensemble de fichiers .build (une étape par fichier),
//! importés par glisser-déposer, depuis un lien Kami-labs ou un lien direct.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::art::USER_AGENT;
use crate::guide;

type Res<T> = Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildMeta {
    pub id: String,
    pub name: String,
    pub source: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    builds: Vec<BuildMeta>,
    active: Option<String>,
    /// Build actif de chaque personnage.
    #[serde(default)]
    by_character: std::collections::HashMap<String, String>,
}

pub struct BuildStore {
    dir: PathBuf,
    index: Index,
    character: Option<String>,
}

/// Un fichier .build lu en mémoire : nom de fichier et contenu.
pub struct BuildFile {
    pub name: String,
    pub data: Vec<u8>,
}

impl BuildStore {
    pub fn open(data_dir: &Path) -> BuildStore {
        let dir = data_dir.join("builds");
        let index = std::fs::read_to_string(dir.join("builds.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        BuildStore { dir, index, character: None }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join("builds.json"), serde_json::to_string_pretty(&self.index).unwrap_or_default());
    }

    pub fn list(&self) -> &[BuildMeta] {
        &self.index.builds
    }

    /// Personnage courant : le build actif est mémorisé pour lui.
    pub fn set_character(&mut self, name: Option<&str>) {
        self.character = name.map(str::to_string);
    }

    pub fn active(&self) -> Option<&BuildMeta> {
        let by_char = self.character.as_ref().and_then(|c| self.index.by_character.get(c));
        let id = by_char.or(self.index.active.as_ref())?;
        self.index.builds.iter().find(|b| &b.id == id)
    }

    pub fn set_active(&mut self, id: &str) {
        self.index.active = Some(id.to_string());
        if let Some(c) = &self.character {
            self.index.by_character.insert(c.clone(), id.to_string());
        }
        self.save();
    }

    pub fn remove(&mut self, id: &str) {
        self.index.builds.retain(|b| b.id != id);
        self.index.by_character.retain(|_, b| b != id);
        let _ = std::fs::remove_dir_all(self.dir.join(id));
        if self.index.active.as_deref() == Some(id) {
            self.index.active = self.index.builds.first().map(|b| b.id.clone());
        }
        self.save();
    }

    /// Dossier du build (ses étapes et ses réglages, comme les choix d'attributs).
    pub fn dir_of(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    /// Fichiers .build du build, triés (Act 1, Act 2… Endgame Step 3).
    pub fn files(&self, id: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.dir.join(id))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "build"))
            .collect();
        files.sort();
        files
    }

    /// Enregistre un nouveau build à partir de ses fichiers et le rend actif.
    pub fn add(&mut self, files: Vec<BuildFile>, source: &str) -> Res<BuildMeta> {
        // On ne garde que les fichiers qui sont bien des étapes de build lisibles.
        let mut steps: Vec<(String, guide::RawBuild, Vec<u8>)> = files
            .into_iter()
            .filter_map(|f| {
                let raw: guide::RawBuild = serde_json::from_slice(&f.data).ok()?;
                guide::split_name(&raw.name).0.as_ref()?;
                Some((f.name, raw, f.data))
            })
            .collect();
        if steps.is_empty() {
            return Err("aucune étape de build reconnue (fichiers .build attendus : Act 1, Act 2… Endgame Step N, ou tranches de niveaux « 1 - 21 - … »)".into());
        }
        steps.sort_by(|a, b| step_order(&a.1.name).cmp(&step_order(&b.1.name)));
        let name = guide::split_name(&steps[0].1.name).1;
        let id = format!("{}-{}", slug(&name), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        let dir = self.dir.join(&id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (i, (_, raw, data)) in steps.iter().enumerate() {
            let label = guide::split_name(&raw.name).0.unwrap_or_default();
            let safe: String = label.chars().map(|c| if c.is_alphanumeric() || " .+-%".contains(c) { c } else { '_' }).collect();
            std::fs::write(dir.join(format!("{:02} - {safe}.build", i + 1)), data).map_err(|e| e.to_string())?;
        }
        let meta = BuildMeta { id: id.clone(), name, source: source.to_string() };
        self.index.builds.push(meta.clone());
        self.set_active(&id);
        Ok(meta)
    }
}

/// Ordre des étapes : tranches de niveaux, actes, endgame numéroté, puis variantes nommées
/// (dans l'ordre de l'archive, le tri étant stable).
fn step_order(name: &str) -> (u32, u32) {
    match guide::split_name(name).0 {
        Some(l) if guide::level_range(&l).is_some() => (0, guide::level_range(&l).map(|r| r.0).unwrap_or(0)),
        Some(l) if l.starts_with("Act ") => (1, l[4..].trim().parse().unwrap_or(0)),
        Some(l) if l.starts_with("Endgame Step ") => (2, l[13..].trim().parse().unwrap_or(0)),
        _ => (3, 0),
    }
}

fn slug(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Fichiers .build contenus dans un fichier .build ou une archive .zip.
pub fn read_build_files(name: &str, data: Vec<u8>) -> Res<Vec<BuildFile>> {
    if name.to_lowercase().ends_with(".zip") || data.starts_with(b"PK") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("archive illisible : {e}"))?;
        let mut out = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            if f.name().to_lowercase().ends_with(".build") {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                out.push(BuildFile { name: f.name().to_string(), data: buf });
            }
        }
        return Ok(out);
    }
    Ok(vec![BuildFile { name: name.to_string(), data }])
}

static KAMI_ZIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https://kami-labs\.fr/wp-content/uploads/poe2-builds/[^"'\s]+\.zip"#).unwrap());
/// Certaines pages n'ont pas d'archive mais des fichiers .build séparés.
static KAMI_BUILD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https://kami-labs\.fr/wp-content/uploads/poe2-builds/[^"'\s]+\.build"#).unwrap());

fn download(url: &str) -> Res<Vec<u8>> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(403, _) => "le site refuse l'accès aux logiciels (403)".to_string(),
            e => e.to_string(),
        })?;
    let mut buf = Vec::new();
    resp.into_reader().take(50_000_000).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Fichiers d'un build à partir d'un lien (page Kami-labs, ou lien direct .build / .zip).
pub fn fetch_url(url: &str) -> Res<Vec<BuildFile>> {
    let url = url.trim();
    let lower = url.to_lowercase();
    if lower.contains("mobalytics.gg") || lower.contains("maxroll.gg") {
        return Err("ce site bloque les logiciels : clique « Download Build File » sur la page, puis glisse le fichier téléchargé ici".into());
    }
    if lower.ends_with(".build") || lower.ends_with(".zip") {
        let name = url.rsplit('/').next().unwrap_or("build.zip").to_string();
        return read_build_files(&name, download(url)?);
    }
    if lower.contains("kami-labs.fr") {
        let page = String::from_utf8_lossy(&download(url)?).into_owned();
        if let Some(zip_url) = KAMI_ZIP.find(&page) {
            return read_build_files(zip_url.as_str(), download(zip_url.as_str())?);
        }
        let mut urls: Vec<&str> = KAMI_BUILD.find_iter(&page).map(|m| m.as_str()).collect();
        urls.sort();
        urls.dedup();
        if urls.is_empty() {
            return Err("pas de fichier de build sur cette page Kami-labs".into());
        }
        let mut files = Vec::new();
        for u in urls {
            let name = u.rsplit('/').next().unwrap_or("etape.build");
            files.extend(read_build_files(name, download(u)?)?);
        }
        return Ok(files);
    }
    Err("lien non pris en charge : page Kami-labs, ou lien direct vers un fichier .build ou .zip".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_slug() {
        assert!(step_order("Act 2 {2} - X") < step_order("Act 10 {10} - X"));
        assert!(step_order("Act 5 {5} - X") < step_order("Endgame Step 1 {6} - X"));
        assert_eq!(slug("Cold Caster Chronomancer"), "cold-caster-chronomancer");
    }

    /// Réseau : page Kami-labs sans archive, avec un fichier .build par tranche de niveaux.
    #[test]
    #[ignore]
    fn kami_page_with_separate_build_files() {
        let files = fetch_url("https://kami-labs.fr/path-of-exile-2/builds-path-of-exile-2/build-guerrier-earthshatter-brutus-warbringer-saison-5/").unwrap();
        let dir = std::env::temp_dir().join(format!("poe2-tracker-kami-{}", std::process::id()));
        let mut store = BuildStore::open(&dir);
        let meta = store.add(files, "test").unwrap();
        let steps = crate::guide::load_steps(&store.files(&meta.id), &crate::guide::Catalog::default());
        eprintln!("{} : {:?}", meta.name, steps.iter().map(|s| &s.label).collect::<Vec<_>>());
        assert_eq!(steps[0].label, "Niv. 1-21");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mobalytics_is_explained() {
        let err = fetch_url("https://mobalytics.gg/poe-2/builds/x").err().unwrap();
        assert!(err.contains("Download Build File"));
    }
}
