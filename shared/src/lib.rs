//! Ce que poe2-tracker et son serveur s'échangent : le profil public d'un personnage (le même
//! contenu que « Exporter le personnage ») et les réponses de l'API.

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
