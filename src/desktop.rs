//! Icône de l'application : celle de la fenêtre, et sous Linux l'entrée du menu des
//! applications (icône et raccourci .desktop), posées par l'application elle-même pour qu'une
//! simple mise à jour suffise. Sous Windows, l'icône est dans l'exécutable (voir build.rs).

use eframe::egui;

const ICON: &[u8] = include_bytes!("../assets/icon-256.png");
/// Identifiant de la fenêtre, qui la relie à son entrée .desktop sous Linux.
pub const APP_ID: &str = "poe2-tracker";

pub fn window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory_with_format(ICON, image::ImageFormat::Png).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

/// Pose l'icône dans le thème de l'utilisateur et l'associe à l'entrée .desktop (créée si elle
/// manque), par son chemin : certains docs (Deepin) ne la trouvent pas par son nom. Rien pour un binaire de développement (dossier `target`).
#[cfg(target_os = "linux")]
pub fn install() {
    let Ok(exe) = std::env::current_exe() else { return };
    if exe.components().any(|c| c.as_os_str() == "target") {
        return;
    }
    let Some(base) = directories::BaseDirs::new() else { return };
    let data = base.data_dir();
    let icon_dir = data.join("icons/hicolor/256x256/apps");
    let icon = icon_dir.join(format!("{APP_ID}.png"));
    if std::fs::read(&icon).ok().as_deref() != Some(ICON) {
        let _ = std::fs::create_dir_all(&icon_dir);
        let _ = std::fs::write(&icon, ICON);
    }
    let apps = data.join("applications");
    let entry = apps.join(format!("{APP_ID}.desktop"));
    let text = match std::fs::read_to_string(&entry) {
        // Entrée existante : on garde ce que l'utilisateur y a mis, sauf l'icône.
        Ok(old) => {
            let mut lines: Vec<String> = old.lines().filter(|l| !l.starts_with("Icon=") && !l.starts_with("StartupWMClass=")).map(str::to_string).collect();
            let at = lines.iter().position(|l| l.trim() == "[Desktop Entry]").map_or(lines.len(), |i| i + 1);
            lines.insert(at, format!("Icon={}", icon.display()));
            lines.insert(at + 1, format!("StartupWMClass={APP_ID}"));
            lines.join("\n") + "\n"
        }
        Err(_) => format!(
            "[Desktop Entry]\nType=Application\nName=poe2-tracker\nComment=Compagnon pour Path of Exile 2\nExec={}\nIcon={}\nStartupWMClass={APP_ID}\nTerminal=false\nCategories=Game;Utility;\n",
            exe.display(),
            icon.display()
        ),
    };
    if std::fs::read_to_string(&entry).ok().as_deref() != Some(text.as_str()) {
        let _ = std::fs::create_dir_all(&apps);
        let _ = std::fs::write(&entry, text);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn install() {}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_decodes() {
        let icon = super::window_icon().unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
    }
}
