//! Recherche du dossier d'installation du jeu (Steam ou version autonome, Linux et Windows).

use std::path::{Path, PathBuf};

const STEAM_GAME: &str = "steamapps/common/Path of Exile 2";

/// Ce que contient un dossier candidat.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GameCheck {
    /// Archives des images (Bundles2 chez Steam, Content.ggpk en version autonome).
    pub assets: bool,
    /// Log du client, d'où viennent personnage, zones et récompenses.
    pub log: bool,
}

impl GameCheck {
    pub fn ok(&self) -> bool {
        self.assets && self.log
    }
}

pub fn check(dir: &Path) -> GameCheck {
    GameCheck {
        assets: dir.join("Bundles2/_.index.bin").is_file() || dir.join("Content.ggpk").is_file(),
        log: log_path(dir).is_file(),
    }
}

pub fn log_path(game_dir: &Path) -> PathBuf {
    game_dir.join("logs").join("Client.txt")
}

/// Dossiers Steam possibles (racines), avant lecture de leurs bibliothèques.
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        if let Some(p) = windows_steam_path() {
            roots.push(p);
        }
        for base in ["C:/Program Files (x86)/Steam", "C:/Program Files/Steam"] {
            roots.push(PathBuf::from(base));
        }
    } else if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) {
        for rel in [".local/share/Steam", ".steam/steam", ".steam/root", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            roots.push(home.join(rel));
        }
    }
    roots
}

#[cfg(windows)]
fn windows_steam_path() -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam").ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path))
}

#[cfg(not(windows))]
fn windows_steam_path() -> Option<PathBuf> {
    None
}

/// Bibliothèques déclarées dans `libraryfolders.vdf` (jeux installés sur un autre disque).
fn steam_libraries(root: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) else { return vec![] };
    library_paths(&text)
}

fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|l| {
            let mut parts = l.split('"').filter(|s| !s.trim().is_empty());
            (parts.next()? == "path").then(|| parts.next().map(|p| PathBuf::from(p.replace("\\\\", "/"))))?
        })
        .collect()
}

/// Tous les emplacements à essayer, du plus probable au moins probable.
pub fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in steam_roots() {
        out.push(root.join(STEAM_GAME));
        out.extend(steam_libraries(&root).into_iter().map(|l| l.join(STEAM_GAME)));
    }
    if cfg!(windows) {
        for drive in ["C", "D", "E", "F", "G"] {
            for rel in [
                "Program Files (x86)/Grinding Gear Games/Path of Exile 2",
                "Program Files/Grinding Gear Games/Path of Exile 2",
                "Grinding Gear Games/Path of Exile 2",
                "Games/Path of Exile 2",
                "SteamLibrary/steamapps/common/Path of Exile 2",
            ] {
                out.push(PathBuf::from(format!("{drive}:/{rel}")));
            }
        }
    }
    out.dedup();
    out
}

/// Premier dossier complet trouvé (archives et log), sinon le premier qui a au moins l'un des deux.
pub fn detect() -> Option<PathBuf> {
    let all = candidates();
    all.iter()
        .find(|d| check(d).ok())
        .or_else(|| all.iter().find(|d| {
            let c = check(d);
            c.assets || c.log
        }))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_library_paths() {
        let vdf = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        assert_eq!(library_paths(vdf), vec![PathBuf::from("C:/Program Files (x86)/Steam"), PathBuf::from("D:/SteamLibrary")]);
    }
}
