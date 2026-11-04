//! Ce que l'app partage avec le serveur de poe2-tracker si le joueur l'a permis (réglage
//! « Envoyer les erreurs ») : ses erreurs, avec la fin de son journal, et les boss des maps lus
//! à l'écran. Les boss déjà connus de tous sont lus au démarrage, sans rien envoyer, pour ne
//! pas relire l'écran d'une map connue.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use poe2_shared::{MapBoss, MapBosses, Report, ReportEntry};

use crate::applog;
use crate::art::USER_AGENT;
use crate::profile::{create_install, error_text, percent, server};

/// Les erreurs en attente partent au plus une fois par minute.
const REPORT_EVERY: Duration = Duration::from_secs(60);
/// Fin du journal jointe à un rapport.
const LOG_TAIL: usize = 32 * 1024;

pub enum Event {
    /// Jeton de l'installation, à garder dans la config.
    Token(String),
    Bosses(MapBosses),
}

#[derive(Default)]
pub struct Share {
    chan: Option<(Sender<Event>, Receiver<Event>)>,
    last_report: Option<Instant>,
    /// Boss connus du serveur : code de la map → nom.
    pub bosses: MapBosses,
    fetched: bool,
}

impl Share {
    fn tx(&mut self) -> Sender<Event> {
        self.chan.get_or_insert_with(channel).0.clone()
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let events: Vec<Event> = self.chan.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::Bosses(b) => {
                    self.bosses.extend(b);
                    None
                }
                e => Some(e),
            })
            .collect()
    }

    /// Boss connus du serveur, lus une fois.
    pub fn fetch_bosses(&mut self) {
        if std::mem::replace(&mut self.fetched, true) {
            return;
        }
        let tx = self.tx();
        std::thread::spawn(move || {
            let url = format!("{}/v1/map-bosses", server());
            match ureq::get(&url).set("User-Agent", USER_AGENT).timeout(Duration::from_secs(20)).call().map_err(error_text).and_then(|r| r.into_json::<MapBosses>().map_err(|e| e.to_string())) {
                Ok(b) => {
                    crate::log_info!("{} boss de maps connus du serveur", b.len());
                    let _ = tx.send(Event::Bosses(b));
                }
                Err(e) => crate::log_warn!("boss des maps du serveur indisponibles : {e}"),
            }
        });
    }

    /// Boss d'une map lu à l'écran, partagé.
    pub fn send_boss(&mut self, token: String, code: String, boss: String) {
        self.bosses.insert(code.clone(), boss.clone());
        let tx = self.tx();
        std::thread::spawn(move || {
            let Some(token) = ensure_token(token, &tx) else { return };
            let url = format!("{}/v1/me/map-bosses/{}", server(), percent(&code));
            if let Err(e) = ureq::put(&url).set("User-Agent", USER_AGENT).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(20)).send_json(MapBoss { boss }) {
                crate::log_warn!("envoi du boss de {code} impossible : {}", error_text(e));
            }
        });
    }

    /// Erreurs en attente envoyées (au plus une fois par minute) ; remises en attente si l'envoi
    /// échoue.
    pub fn report(&mut self, token: String, character: Option<String>) {
        if self.last_report.is_some_and(|t| t.elapsed() < REPORT_EVERY) {
            return;
        }
        self.last_report = Some(Instant::now());
        let pending = applog::take_pending();
        if pending.is_empty() {
            return;
        }
        let tx = self.tx();
        std::thread::spawn(move || {
            let entries = pending.iter().map(|e| ReportEntry { at: e.at, level: format!("{:?}", e.level).to_uppercase(), message: e.message.clone() }).collect();
            let report = Report { app_version: env!("CARGO_PKG_VERSION").into(), os: std::env::consts::OS.into(), character, entries, log_tail: applog::tail(LOG_TAIL) };
            let sent = ensure_token(token, &tx).map(|token| {
                ureq::post(&format!("{}/v1/me/reports", server())).set("User-Agent", USER_AGENT).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(30)).send_json(&report)
            });
            match sent {
                Some(Ok(_)) => {}
                // Pas de log_warn ici : il repartirait dans le rapport suivant, en boucle.
                Some(Err(e)) => {
                    eprintln!("rapport d'erreurs non envoyé : {}", error_text(e));
                    applog::restore_pending(pending);
                }
                None => applog::restore_pending(pending),
            }
        });
    }
}

/// Jeton de l'installation, demandé au serveur s'il n'y en a pas encore.
fn ensure_token(token: String, tx: &Sender<Event>) -> Option<String> {
    if !token.is_empty() {
        return Some(token);
    }
    match create_install() {
        Ok(t) => {
            let _ = tx.send(Event::Token(t.clone()));
            Some(t)
        }
        Err(e) => {
            eprintln!("installation non enregistrée : {}", error_text(e));
            None
        }
    }
}
