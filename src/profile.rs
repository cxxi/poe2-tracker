//! Profil public : le personnage (le même contenu que « Exporter le personnage ») envoyé au
//! serveur de poe2-tracker quand il change, lisible ensuite par un lien public. Les requêtes
//! partent en arrière-plan ; leurs résultats reviennent par `poll`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use poe2_shared::{InstallCreated, Profile, ProfileSaved};

use crate::art::USER_AGENT;

/// Le profil est regardé (et envoyé s'il a changé) au plus toutes les 30 secondes.
const EVERY: Duration = Duration::from_secs(30);

/// Adresse du serveur (`POE2_TRACKER_SERVER` pour en essayer un autre).
pub fn server() -> String {
    std::env::var("POE2_TRACKER_SERVER").unwrap_or_else(|_| "https://poe2-tracker.cxxi.io".into())
}

/// Lien public d'un profil : l'export en Markdown, à donner tel quel à une IA.
pub fn public_url(id: &str) -> String {
    format!("{}/v1/profiles/{id}.md", server())
}

pub enum Event {
    /// Jeton de l'installation, à garder dans la config (vide : refusé par le serveur, à refaire).
    Token(String),
    /// Profil enregistré : son identifiant public.
    Saved { character: String, id: String },
    /// Profils retirés du serveur (profil public décoché).
    Deleted,
    Failed(String),
}

/// Résultat d'une requête, avec l'empreinte du profil envoyé.
struct Done {
    events: Vec<Event>,
    sent: Option<(String, u64)>,
}

pub struct Sync {
    tx: Sender<Done>,
    rx: Receiver<Done>,
    busy: bool,
    last_check: Option<Instant>,
    /// Empreinte du dernier profil envoyé, par personnage : rien ne repart s'il n'a pas changé.
    sent: HashMap<String, u64>,
    /// Dernier résultat, pour les réglages : texte, et s'il s'agit d'une erreur.
    pub status: Option<(String, bool)>,
}

impl Default for Sync {
    fn default() -> Self {
        let (tx, rx) = channel();
        Sync { tx, rx, busy: false, last_check: None, sent: HashMap::new(), status: None }
    }
}

fn fingerprint(p: &Profile) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&p.markdown, &p.lang, &p.character.level).hash(&mut h);
    h.finish()
}

pub fn error_text(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let msg = r.into_json::<poe2_shared::ApiError>().map(|e| e.error).unwrap_or_default();
            tr!("le serveur répond {code} {msg}", code, msg)
        }
        ureq::Error::Transport(t) => tr!("serveur injoignable ({e})", e = t.to_string()),
    }
}

pub fn create_install() -> Result<String, ureq::Error> {
    let r: InstallCreated = ureq::post(&format!("{}/v1/installs", server())).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call()?.into_json()?;
    Ok(r.token)
}

fn put(token: &str, profile: &Profile) -> Result<String, ureq::Error> {
    let url = format!("{}/v1/me/profiles/{}", server(), percent(&profile.character.name));
    let r: ProfileSaved = ureq::put(&url)
        .set("User-Agent", USER_AGENT)
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(Duration::from_secs(20))
        .send_json(profile)?
        .into_json()?;
    Ok(r.id)
}

/// Nom de personnage dans un chemin d'URL.
pub fn percent(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

impl Sync {
    /// Le moment de regarder si le profil a changé (rien en cours, 30 s depuis la dernière fois).
    pub fn due(&mut self) -> bool {
        if self.busy || self.last_check.is_some_and(|t| t.elapsed() < EVERY) {
            return false;
        }
        self.last_check = Some(Instant::now());
        true
    }

    /// Le prochain regard tout de suite (pièce suivie en jeu), sans attendre les 30 s ; envoyé
    /// seulement s'il a changé. Un envoi en cours : le regard suit sa fin.
    pub fn soon(&mut self) {
        self.last_check = None;
    }

    /// Le prochain regard tout de suite, et le profil renvoyé même inchangé (profil public coché).
    pub fn force(&mut self) {
        self.last_check = None;
        self.sent.clear();
    }

    /// Envoie le profil s'il a changé depuis le dernier envoi ; sans jeton, l'installation est
    /// d'abord enregistrée auprès du serveur.
    pub fn send(&mut self, token: String, profile: Profile) {
        let hash = fingerprint(&profile);
        let name = profile.character.name.clone();
        if self.sent.get(&name) == Some(&hash) {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut events = Vec::new();
            let token = if token.is_empty() {
                match create_install() {
                    Ok(t) => {
                        events.push(Event::Token(t.clone()));
                        t
                    }
                    Err(e) => {
                        let e = error_text(e);
                        crate::log_warn!("profil public : création de l'installation impossible : {e}");
                        events.push(Event::Failed(e));
                        let _ = tx.send(Done { events, sent: None });
                        return;
                    }
                }
            } else {
                token
            };
            let sent = match put(&token, &profile) {
                Ok(id) => {
                    events.push(Event::Saved { character: name.clone(), id });
                    Some((name, hash))
                }
                Err(e) => {
                    // Jeton inconnu du serveur (base refaite…) : on en redemandera un.
                    if matches!(e, ureq::Error::Status(401, _)) {
                        events.push(Event::Token(String::new()));
                    }
                    let e = error_text(e);
                    crate::log_warn!("profil public : envoi de {name} impossible : {e}");
                    events.push(Event::Failed(e));
                    None
                }
            };
            let _ = tx.send(Done { events, sent });
        });
    }

    /// Retire du serveur les profils de ces personnages (profil public décoché).
    pub fn delete(&mut self, token: String, characters: Vec<String>) {
        self.sent.clear();
        if token.is_empty() {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut failed = None;
            for c in &characters {
                let url = format!("{}/v1/me/profiles/{}", server(), percent(c));
                if let Err(e) = ureq::delete(&url).set("User-Agent", USER_AGENT).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(20)).call() {
                    let e = error_text(e);
                    crate::log_warn!("profil public : suppression de {c} impossible : {e}");
                    failed = Some(e);
                }
            }
            let events = vec![failed.map_or(Event::Deleted, Event::Failed)];
            let _ = tx.send(Done { events, sent: None });
        });
    }

    /// Résultats des requêtes terminées.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(done) = self.rx.try_recv() {
            self.busy = false;
            if let Some((name, hash)) = done.sent {
                self.sent.insert(name, hash);
            }
            for e in &done.events {
                self.status = match e {
                    Event::Saved { .. } => Some((tr!("profil à jour sur le serveur").to_string(), false)),
                    Event::Deleted => Some((tr!("profil retiré du serveur").to_string(), false)),
                    Event::Failed(msg) => Some((msg.clone(), true)),
                    Event::Token(_) => continue,
                };
            }
            out.extend(done.events);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_names_in_urls() {
        assert_eq!(percent("Juliana_novachrono"), "Juliana_novachrono");
        assert_eq!(percent("Zoé"), "Zo%C3%A9");
    }
}
