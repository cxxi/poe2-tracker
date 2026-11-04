//! Ce que poe2-tracker et son serveur s'échangent : le profil public d'un personnage (le même
//! contenu que « Exporter le personnage »), les boss des maps lus à l'écran, les rapports
//! d'erreurs et les réponses de l'API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version du format de `Profile` : augmentée à chaque changement incompatible.
pub const PROFILE_FORMAT: u32 = 1;

/// Profil public d'un personnage, envoyé par l'app à chaque changement.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub format: u32,
    /// Langue des textes de l'app (« fr » ou « en ») ; les textes du jeu restent en anglais.
    pub lang: String,
    pub app_version: String,
    pub character: CharacterInfo,
    /// L'export complet en Markdown, tel que l'app le copie.
    pub markdown: String,
    /// Le même export, section par section.
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterInfo {
    pub name: String,
    pub class: String,
    pub level: u32,
    /// Où en est le personnage : acte, interludes ou endgame.
    pub progress: String,
    pub league: Option<String>,
    pub build: Option<BuildInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub name: String,
    pub url: String,
    /// Étape du build affichée.
    pub step: Option<String>,
}

/// Une section de l'export : `key` stable (gear, stats, skills, passives, jewels, atlas, maps,
/// quests), titre dans la langue de l'app, contenu en Markdown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub key: String,
    pub title: String,
    pub markdown: String,
}

/// Réponse à la création d'une installation : son jeton secret, montré une seule fois.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallCreated {
    pub token: String,
}

/// Réponse à l'envoi d'un profil : son identifiant public, stable pour ce personnage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileSaved {
    pub id: String,
}

/// Profil lu sur la route publique.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicProfile {
    pub id: String,
    /// Dernière mise à jour, en secondes depuis l'époque Unix.
    pub updated_at: i64,
    pub profile: Profile,
}

/// Erreur renvoyée par l'API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

/// Boss d'un genre de map, lu à l'écran par l'app (`PUT /v1/me/map-bosses/{code}`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapBoss {
    pub boss: String,
}

/// Boss connus des maps (`GET /v1/map-bosses`) : code de la map (« MapSlick ») → nom du boss le
/// plus signalé.
pub type MapBosses = BTreeMap<String, String>;

/// Rapport d'erreurs de l'app (`POST /v1/me/reports`), envoyé si le joueur l'a permis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub app_version: String,
    /// « linux », « windows ».
    pub os: String,
    /// Personnage actif, pour savoir de qui vient le rapport.
    pub character: Option<String>,
    pub entries: Vec<ReportEntry>,
    /// Fin du journal de l'app, pour le contexte.
    pub log_tail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReportEntry {
    /// Secondes Unix.
    pub at: i64,
    /// « WARN » ou « ERROR ».
    pub level: String,
    pub message: String,
}
