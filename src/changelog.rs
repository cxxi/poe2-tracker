//! Nouveautés de chaque version (CHANGELOG.md, intégré au programme), montrées après une mise
//! à jour et depuis le menu.

const TEXT: &str = include_str!("../CHANGELOG.md");
const TEXT_EN: &str = include_str!("../CHANGELOG.en.md");

/// Une version et ses changements.
pub struct Release {
    pub version: String,
    pub items: Vec<String>,
}

/// Toutes les versions, de la plus récente à la plus ancienne, dans la langue de l'interface.
pub fn releases() -> Vec<Release> {
    parse(if crate::i18n::english() { TEXT_EN } else { TEXT })
}

fn parse(text: &str) -> Vec<Release> {
    let mut out: Vec<Release> = Vec::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("## ") {
            out.push(Release { version: v.trim().to_string(), items: Vec::new() });
        } else if let (Some(item), Some(r)) = (line.strip_prefix("- "), out.last_mut()) {
            r.items.push(item.trim().to_string());
        }
    }
    out
}

/// Versions arrivées depuis `seen`, jusqu'à la version en cours comprise.
pub fn since(seen: &str, current: &str) -> Vec<Release> {
    releases().into_iter().filter(|r| crate::updater::newer(&r.version, seen) && !crate::updater::newer(&r.version, current)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_is_read_and_filtered() {
        let all = releases();
        assert!(all.len() >= 5 && all.iter().all(|r| !r.items.is_empty()));
        assert_eq!(all.last().unwrap().version, "0.2.0");
        let new: Vec<String> = since("0.2.1", "0.2.3").into_iter().map(|r| r.version).collect();
        assert_eq!(new, ["0.2.3", "0.2.2"]);
        assert!(since("0.2.3", "0.2.3").is_empty());
    }

    /// La version du programme a ses nouveautés.
    #[test]
    fn current_version_is_documented() {
        let v = crate::updater::VERSION;
        assert!(releases().iter().any(|r| r.version == v), "CHANGELOG.md : section « ## {v} » manquante");
    }

    /// Les nouveautés en anglais suivent celles en français, version par version.
    #[test]
    fn english_changelog_matches() {
        let (fr, en) = (parse(TEXT), parse(TEXT_EN));
        let versions = |r: &[Release]| r.iter().map(|r| (r.version.clone(), r.items.len())).collect::<Vec<_>>();
        assert_eq!(versions(&fr), versions(&en), "CHANGELOG.en.md ne suit pas CHANGELOG.md");
    }
}
