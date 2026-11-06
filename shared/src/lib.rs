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
    /// Compte Path of Exile du joueur (« Pseudo#1234 »), s'il l'a donné à l'app.
    #[serde(default)]
    pub account: Option<String>,
    /// Dernière mise à jour, en secondes depuis l'époque Unix.
    pub updated_at: i64,
    pub profile: Profile,
}

/// Compte Path of Exile d'une installation (`PUT /v1/me/account`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub account: String,
}

/// Compte Path of Exile bien formé : lettres, chiffres ou « _ », puis « # » et quatre chiffres
/// (« cxxi#2432 »).
pub fn valid_account(s: &str) -> bool {
    let Some((name, tag)) = s.split_once('#') else { return false };
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && tag.len() == 4 && tag.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_format() {
        assert!(valid_account("cxxi#2432"));
        assert!(valid_account("THEALIASFMR#3293"));
        assert!(valid_account("Gamer_42#0007"));
        for bad in ["", "cxxi", "cxxi#243", "cxxi#24321", "#2432", "cx xi#2432", "cxxi#24a2", "cxxi#2432#1", "Zoé#1234"] {
            assert!(!valid_account(bad), "{bad}");
        }
    }
}

/// Objet vendu par le joueur (`PUT /v1/me/sales/{annonce}`) : une annonce de sa boutique qu'il a
/// marquée « vendu ». Pour les estimations à venir : ce qui part, à quel prix, en combien de temps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sale {
    pub league: String,
    /// Classe d'objet comme dans le texte en jeu (« Body Armours »).
    pub item_class: String,
    pub rarity: String,
    pub name: String,
    pub base: String,
    pub item_level: u32,
    /// Propriétés, implicites et autres mods, tels que le site les donne.
    pub properties: Vec<String>,
    pub implicits: Vec<String>,
    pub mods: Vec<String>,
    /// Dernier prix demandé (« 70 », « exalted »).
    pub amount: f64,
    pub currency: String,
    /// Mise en vente (date la plus ancienne donnée par le site) et dernière date du site, en
    /// secondes Unix.
    pub listed_at: Option<i64>,
    pub indexed: Option<i64>,
    /// Lecture où l'annonce n'était plus en vente.
    pub gone_at: i64,
}

/// Identifiant d'annonce du site de trade : 64 caractères hexadécimaux.
pub fn valid_listing_id(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
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

/// Code de map bien formé (« MapSlick », « MapUberBoss_JadeCitadel ») : lettres, chiffres et
/// « _ », 80 au plus.
pub fn valid_map_code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Ce qu'une installation sait d'une map faite en groupe. Les heures sont en secondes Unix (UTC),
/// déjà corrigées du décalage de l'heure du journal.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapPresence {
    /// Personnage qui y est.
    pub character: String,
    /// Niveau de la zone et nom affiché par le jeu.
    pub level: u32,
    pub zone: Option<String>,
    /// Première entrée et dernière sortie (ou dernière activité).
    pub start: i64,
    pub last: i64,
    /// Dedans depuis cette heure, ou dehors.
    pub inside_since: Option<i64>,
    /// Temps passé dedans hors AFK, présence en cours non comprise.
    pub active_secs: i64,
    /// Temps de la map pour le groupe à l'envoi : il avance tant qu'au moins un joueur est
    /// dedans.
    #[serde(default)]
    pub group_secs: i64,
    pub deaths: u32,
    /// Entrées depuis la ville : 1 + retours.
    pub entries: u32,
    /// Ce joueur l'a lancée au device.
    pub opened: bool,
    /// « Map Completed » vu à l'écran.
    pub completed_at: Option<i64>,
    pub boss: Option<String>,
    /// Waystone copié avant d'y entrer : seul celui qui l'a lancée l'a.
    pub waystone: Option<MapWaystone>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapWaystone {
    pub name: String,
    pub tier: u32,
    /// En-tête : « Item Rarity: +33% »…
    pub stats: Vec<String>,
    pub mods: Vec<String>,
    pub corrupted: bool,
}

/// Synchro des maps en groupe (`POST /v1/me/map-sync`) : la map où est l'installation, s'il y en
/// a une. La réponse donne les autres joueurs de cette map et les maps en cours de ceux avec qui
/// elle a fait une map ces dernières heures.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapSync {
    pub map: Option<MapAt>,
}

/// Une instance de map : son code et sa graine (la même pour tous les joueurs qui y sont).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapAt {
    pub code: String,
    pub seed: u64,
    pub presence: MapPresence,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapSyncReply {
    pub maps: Vec<GroupMap>,
}

/// Une map et ce qu'en disent les autres joueurs (jamais l'installation qui demande).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupMap {
    pub code: String,
    pub seed: u64,
    pub members: Vec<MapMember>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapMember {
    pub presence: MapPresence,
    /// Dernier envoi de ce joueur, en secondes Unix.
    pub updated_at: i64,
}
