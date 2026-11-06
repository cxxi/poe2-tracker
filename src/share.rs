//! Ce que l'app partage avec le serveur de poe2-tracker si le joueur l'a permis (réglage
//! « Envoyer les erreurs ») : ses erreurs, avec la fin de son journal, et les boss des maps lus
//! à l'écran. Les boss déjà connus de tous sont lus au démarrage, sans rien envoyer, pour ne
//! pas relire l'écran d'une map connue. Le compte Path of Exile du joueur part toujours : les
//! profils se lisent par compte. En groupe (réglage « Maps en groupe »), la map en cours part
//! aussi, pour la voir avec les temps, morts et waystone des coéquipiers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use poe2_shared::{Account, GroupMap, MapAt, MapBoss, MapBosses, MapSync, MapSyncReply, Report, ReportEntry, Sale};

use crate::applog;
use crate::art::USER_AGENT;
use crate::profile::{create_install, error_text, percent, server};

/// Les erreurs en attente partent au plus une fois par minute.
const REPORT_EVERY: Duration = Duration::from_secs(60);
/// Fin du journal jointe à un rapport.
const LOG_TAIL: usize = 32 * 1024;
/// Compte refusé ou serveur injoignable : nouvel essai au plus une fois par minute.
const ACCOUNT_RETRY: Duration = Duration::from_secs(60);
/// Maps en groupe : une synchro toutes les 5 secondes dans une map en groupe (pour savoir vite
/// qui entre ou sort), toutes les 15 secondes sinon (seulement suivre les coéquipiers).
const MAP_SYNC_INSIDE: Duration = Duration::from_secs(5);
const MAP_SYNC_EVERY: Duration = Duration::from_secs(15);
/// La synchro des maps échoue (déjà signalé dans le journal).
static MAP_SYNC_FAILING: AtomicBool = AtomicBool::new(false);

pub enum Event {
    /// Jeton de l'installation, à garder dans la config.
    Token(String),
    Bosses(MapBosses),
    /// Compte enregistré par le serveur pour ce jeton.
    Account { token: String, account: String },
    /// Vente enregistrée (ou retirée) par le serveur : identifiant de l'annonce, et si elle y est.
    Sale { listing: String, kept: bool },
    /// Maps des coéquipiers : celle où l'on est, et celles qu'ils viennent de lancer.
    Maps(Vec<GroupMap>),
}

#[derive(Default)]
pub struct Share {
    chan: Option<(Sender<Event>, Receiver<Event>)>,
    last_report: Option<Instant>,
    last_account: Option<Instant>,
    last_sale: Option<Instant>,
    last_map_sync: Option<Instant>,
    /// Dernière réponse de la synchro des maps en groupe (vide hors groupe).
    pub group_maps: Vec<GroupMap>,
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
                Event::Maps(m) => {
                    self.group_maps = m;
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

    /// Compte changé : envoyé sans attendre la minute.
    pub fn retry_account(&mut self) {
        self.last_account = None;
    }

    /// Compte Path of Exile envoyé au serveur (au plus une requête par minute).
    pub fn send_account(&mut self, token: String, account: String) {
        if self.last_account.is_some_and(|t| t.elapsed() < ACCOUNT_RETRY) {
            return;
        }
        self.last_account = Some(Instant::now());
        let tx = self.tx();
        std::thread::spawn(move || {
            let Some(token) = ensure_token(token, &tx) else { return };
            let url = format!("{}/v1/me/account", server());
            match ureq::put(&url).set("User-Agent", USER_AGENT).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(20)).send_json(Account { account: account.clone() }) {
                Ok(_) => {
                    let _ = tx.send(Event::Account { token, account });
                }
                Err(e) => {
                    // Jeton inconnu du serveur (base refaite…) : on en redemandera un.
                    if matches!(e, ureq::Error::Status(401, _)) {
                        let _ = tx.send(Event::Token(String::new()));
                    }
                    crate::log_warn!("envoi du compte impossible : {}", error_text(e));
                }
            }
        });
    }

    /// Vente à envoyer (`Some`) ou à retirer (`None`) du serveur : une requête toutes les
    /// 5 secondes au plus, celles en attente repassent à l'image suivante.
    pub fn send_sale(&mut self, token: String, listing: String, sale: Option<Sale>) {
        if self.last_sale.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        self.last_sale = Some(Instant::now());
        let tx = self.tx();
        std::thread::spawn(move || {
            let Some(token) = ensure_token(token, &tx) else { return };
            let url = format!("{}/v1/me/sales/{listing}", server());
            let auth = format!("Bearer {token}");
            let kept = sale.is_some();
            let sent = match sale {
                Some(s) => ureq::put(&url).set("User-Agent", USER_AGENT).set("Authorization", &auth).timeout(Duration::from_secs(20)).send_json(s),
                None => ureq::delete(&url).set("User-Agent", USER_AGENT).set("Authorization", &auth).timeout(Duration::from_secs(20)).call(),
            };
            match sent {
                Ok(_) => {
                    let _ = tx.send(Event::Sale { listing, kept });
                }
                Err(e) => crate::log_warn!("vente {listing} non envoyée : {}", error_text(e)),
            }
        });
    }

    /// Map en cours envoyée (ou rien, pour seulement suivre les coéquipiers) ; la réponse arrive
    /// par `poll` dans `group_maps`. Au plus une requête toutes les 5 secondes dans une map en
    /// groupe, toutes les 15 secondes sinon.
    pub fn map_sync(&mut self, token: String, map: Option<MapAt>) {
        let every = if map.as_ref().is_some_and(|m| m.presence.group_secs > 0 || m.presence.inside_since.is_some()) { MAP_SYNC_INSIDE } else { MAP_SYNC_EVERY };
        if self.last_map_sync.is_some_and(|t| t.elapsed() < every) {
            return;
        }
        self.last_map_sync = Some(Instant::now());
        let tx = self.tx();
        std::thread::spawn(move || {
            let Some(token) = ensure_token(token, &tx) else { return };
            let url = format!("{}/v1/me/map-sync", server());
            let sent = ureq::post(&url).set("User-Agent", USER_AGENT).set("Authorization", &format!("Bearer {token}")).timeout(Duration::from_secs(10)).send_json(MapSync { map });
            match sent.map_err(|e| {
                if matches!(e, ureq::Error::Status(401, _)) {
                    let _ = tx.send(Event::Token(String::new()));
                }
                error_text(e)
            }).and_then(|r| r.into_json::<MapSyncReply>().map_err(|e| e.to_string())) {
                Ok(reply) => {
                    MAP_SYNC_FAILING.store(false, Ordering::Relaxed);
                    let _ = tx.send(Event::Maps(reply.maps));
                }
                // Signalé une fois par panne, pas toutes les 15 secondes.
                Err(e) if !MAP_SYNC_FAILING.swap(true, Ordering::Relaxed) => crate::log_warn!("synchro des maps en groupe impossible : {e}"),
                Err(_) => {}
            }
        });
    }

    /// Plus en groupe (ou réglage coupé) : on oublie les maps des coéquipiers.
    pub fn stop_map_sync(&mut self) {
        self.group_maps.clear();
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
