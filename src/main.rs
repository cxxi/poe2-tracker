//! poe2-tracker : suivi de l'équipement et diagnostic des objets pour Path of Exile 2.
//!
//! En jeu : Ctrl+C sur un objet pour le diagnostiquer, Ctrl+Alt+C pour l'ajouter à
//! l'équipement suivi.

// Sous Windows, pas de console noire à côté de la fenêtre.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod art;
mod augments;
mod builds;
mod bundles;
mod capture;
mod changelog;
mod coach;
mod compare;
mod desktop;
mod gear;
mod gem_info;
mod guide;
mod item;
mod leagues;
mod logwatch;
mod passive_icons;
mod paths;
mod prices;
mod route;
mod rules;
mod stats;
mod trade;
mod ui;
mod ui_art;
mod uniques;
mod updater;
mod vendor_regex;
mod views;
mod weights;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, Vec2};
use serde::{Deserialize, Serialize};

use capture::{CopyEvent, CopyKind};
use gear::{Equipped, Gear, Slot};
use item::Item;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    /// Ligue par défaut (vide = première ligue en cours) ; chaque personnage peut avoir la sienne.
    league: String,
    /// Dossier d'installation du jeu (vide = recherche automatique).
    game_dir: PathBuf,
    /// Valeur (en exalts) à partir de laquelle un objet mérite d'aller au marché.
    market_min_ex: f64,
    /// Log du client (vide = logs/Client.txt dans le dossier du jeu).
    log_path: PathBuf,
    /// L'écran d'accueil a été validé une fois.
    welcomed: bool,
    /// Dernière version dont les nouveautés ont été vues.
    seen_version: String,
    /// Recherche automatique des nouvelles versions sur GitHub.
    auto_update: bool,
    /// Estimation du prix des objets copiés sur le site de trade.
    trade_prices: bool,
    /// Ctrl+C lance l'estimation sur le trade tout de suite (sinon bouton « Estimer »).
    auto_estimate: bool,
    /// Ctrl+C ouvre le détail du diagnostic (sinon l'objet s'ajoute à l'historique).
    open_on_copy: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { league: String::new(), game_dir: PathBuf::new(), market_min_ex: 10.0, log_path: PathBuf::new(), welcomed: false, seen_version: String::new(),
            auto_update: true,
            trade_prices: true,
            auto_estimate: true,
            open_on_copy: true,
        }
    }
}

impl Config {
    fn path(dirs: &ProjectDirs) -> PathBuf {
        dirs.config_dir().join("config.json")
    }

    fn load(dirs: &ProjectDirs) -> Config {
        std::fs::read_to_string(Config::path(dirs)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, dirs: &ProjectDirs) {
        let _ = std::fs::create_dir_all(dirs.config_dir());
        let _ = std::fs::write(Config::path(dirs), serde_json::to_string_pretty(self).unwrap_or_default());
    }

    /// Copie avec les chemins laissés vides remplis par la recherche automatique.
    fn resolved(&self) -> Config {
        let mut c = self.clone();
        if c.game_dir.as_os_str().is_empty() {
            c.game_dir = paths::detect().unwrap_or_default();
        }
        if self.log_path.as_os_str().is_empty() {
            c.log_path = paths::log_path(&c.game_dir);
        }
        c
    }

    /// Ligue pour les outils en ligne de commande : réglage, sinon ligue en cours connue.
    fn cli_league(&self, cache: &std::path::Path) -> String {
        if !self.league.is_empty() {
            return self.league.clone();
        }
        let known: Vec<String> = std::fs::read_to_string(cache.join("leagues.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        known.into_iter().find(|l| !l.contains("Standard") && !l.contains("Hardcore") && !l.starts_with("HC ")).unwrap_or_else(|| "Standard".into())
    }
}

/// Outils en ligne de commande pour explorer les archives du jeu.
fn asset_tools(args: &[String], config: &Config) -> Option<Result<(), Box<dyn std::error::Error>>> {
    let cmd = args.get(1)?.as_str();
    if cmd != "--assets" && cmd != "--extract" {
        return None;
    }
    Some((|| {
        let index = bundles::Index::open(&config.game_dir)?;
        let (hits, n) = index.hash_check(2000);
        eprintln!("{} chemins, contrôle du hash : {hits}/{n}", index.paths.len());
        if cmd == "--assets" {
            let filter = args.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
            for p in index.paths.iter().filter(|p| p.to_lowercase().contains(&filter)) {
                println!("{p}");
            }
        } else {
            let (path, out) = (args.get(2).ok_or("chemin attendu")?, args.get(3).ok_or("sortie attendue")?);
            let data = index.read(path)?;
            if out.ends_with(".png") {
                bundles::decode_dds(&data)?.save(out)?;
            } else {
                std::fs::write(out, &data)?;
            }
            eprintln!("{path} -> {out} ({} octets)", data.len());
        }
        Ok(())
    })())
}

struct App {
    gear: Gear,
    assets: art::Assets,
    ui_art: ui_art::UiArt,
    prices: prices::Prices,
    trade: trade::Trade,
    augments: augments::Augments,
    gem_infos: gem_info::GemInfos,
    /// Augments reconnus par texte d'objet, et suggestions par (texte, étape, niveau).
    socketed: HashMap<String, augments::Socketed>,
    socket_tips: HashMap<(String, String, u32), Vec<String>>,
    /// Catalogue d'augments pour lequel ces caches valent.
    catalog_id: usize,
    /// Runes recommandées du build actif (identifiant du build, étapes).
    build_runes: Option<(String, Vec<builds::StepRunes>)>,
    /// Choix de récompenses de quête du build actif.
    build_quests: Option<(String, Vec<builds::QuestChoice>)>,
    /// Fenêtre des nouveautés ouverte : versions à montrer.
    changelog: Option<Vec<changelog::Release>>,
    /// Étape regardée en aperçu (autre que celle de la progression), non enregistrée.
    step_preview: Option<String>,
    market_min_ex: f64,
    copies: Receiver<CopyEvent>,
    /// Dernier objet diagnostiqué (Ctrl+C).
    diagnosed: Option<Item>,
    /// Historique des objets diagnostiqués, le plus récent en premier (conservé sur disque).
    history: Vec<Item>,
    history_path: PathBuf,
    /// Développement : diagnostic à ouvrir au démarrage (`POE2_TRACKER_DIAG` = rang dans
    /// l'historique), une fois le personnage connu.
    open_diag: Option<usize>,
    status: Option<(String, Instant)>,
    coach: Option<coach::Coach>,
    coach_rx: Receiver<coach::Coach>,
    log_rx: Receiver<logwatch::Event>,
    feed: VecDeque<coach::FeedLine>,
    builds: builds::BuildStore,
    add_build: AddBuild,
    data_dir: PathBuf,
    /// Personnage dont l'équipement, l'historique et le build sont affichés.
    character: Option<String>,
    /// Hauteur réelle de la barre personnage / build (mesurée à l'affichage).
    bar_h: f32,
    tab: views::Tab,
    /// Onglet du panneau de droite.
    side_tab: views::SideTab,
    /// Détail de l'estimation ouvert, pour l'objet de ce texte (troisième niveau : historique,
    /// diagnostic, estimation). Sans effet quand un autre objet est diagnostiqué.
    estimate_open: Option<String>,
    /// Onglet Marché : catégorie poe.ninja affichée et recherche.
    market_category: String,
    market_search: String,
    /// Ligue regardée dans l'onglet Marché quand ce n'est pas celle du personnage, et ses cours.
    market_league: Option<(String, prices::Prices)>,
    /// Étape affichée dans les onglets du build (`None` = étape actuelle).
    view_step: Option<usize>,
    tree: views::TreeView,
    passive_icons: passive_icons::PassiveIcons,
    dirs: ProjectDirs,
    /// Réglages tels qu'enregistrés (chemins vides = recherche automatique)…
    stored: Config,
    /// … et tels qu'utilisés, chemins remplis.
    config: Config,
    leagues: leagues::Leagues,
    /// Ligue utilisée pour les prix (celle du personnage).
    league: String,
    welcome: Welcome,
    /// Fenêtre des réglages ouverte : ses valeurs en cours de modification.
    settings: Option<SettingsDraft>,
    updater: updater::Updater,
}

/// Réglages en cours de modification : appliqués et enregistrés par « Enregistrer », oubliés
/// par « Annuler ».
struct SettingsDraft {
    game_dir: PathBuf,
    trade_prices: bool,
    auto_estimate: bool,
    open_on_copy: bool,
    auto_update: bool,
    market_min_ex: f64,
    /// Choix du dossier du jeu en cours (fenêtre du système).
    picking: Option<Receiver<Option<PathBuf>>>,
}

impl SettingsDraft {
    fn new(stored: &Config, config: &Config) -> SettingsDraft {
        SettingsDraft {
            game_dir: config.game_dir.clone(),
            trade_prices: stored.trade_prices,
            auto_estimate: stored.auto_estimate,
            open_on_copy: stored.open_on_copy,
            auto_update: stored.auto_update,
            market_min_ex: stored.market_min_ex,
            picking: None,
        }
    }
}

/// Écran d'accueil du premier lancement, aussi utilisé pour changer le dossier du jeu.
#[derive(Default)]
struct Welcome {
    open: bool,
    path: String,
    picking: Option<Receiver<Option<PathBuf>>>,
    message: Option<String>,
}

/// Dossier des données propres à un personnage (équipement suivi, historique).
fn character_dir(data_dir: &std::path::Path, name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    data_dir.join("characters").join(safe)
}

/// Gestionnaire de builds : liste, suppression, ajout.
#[derive(Default)]
struct AddBuild {
    open: bool,
    url: String,
    message: Option<(bool, String)>,
    pending: Option<Receiver<Result<Vec<builds::BuildFile>, String>>>,
    /// Build dont la suppression attend confirmation (sur sa ligne).
    confirm_remove: Option<String>,
    /// Build sans lien auquel on en donne un : lien saisi.
    link_for: Option<String>,
    link_url: String,
    /// Mise à jour en cours d'un build depuis sa page : build, lien, fichiers téléchargés.
    link_pending: Option<(String, String, Receiver<Result<Vec<builds::BuildFile>, String>>)>,
}

const FEED_MAX: usize = 60;
/// Largeur minimale du panneau Diagnostic et hauteur minimale du fil d'événements.
const SIDE_MIN: f32 = 300.0;
/// Hauteur de la ligne d'onglets sous la zone d'affichage.
const TAB_H: f32 = 34.0;
const FEED_MIN: f32 = 300.0;
const HISTORY_MAX: usize = 50;

/// « Mobalytics · cold-caster-chronomancer-ky » : site et dernier élément du lien d'un build.
fn short_source(url: &str) -> String {
    let site = match url {
        u if u.contains("mobalytics.gg") => "Mobalytics",
        u if u.contains("kami-labs.fr") => "Kami-labs",
        u => u.split('/').nth(2).unwrap_or(u),
    };
    let path = url.split(['?', '#']).next().unwrap_or(url).trim_end_matches('/');
    format!("{site} · {}", path.rsplit('/').next().unwrap_or(path))
}

/// Type d'objet lisible : emplacement pour une pièce d'équipement, classe du jeu sinon (rune,
/// devise…).
fn kind_label(it: &Item) -> String {
    match gear::kind_of(&it.item_class) {
        Some(gear::Kind::Single(s)) => s.label().to_string(),
        Some(gear::Kind::TwoHanded) => "Arme à deux mains".to_string(),
        Some(gear::Kind::Ring) => "Anneau".to_string(),
        Some(gear::Kind::Charm) => "Charme".to_string(),
        None => it.item_class.clone(),
    }
}

fn load_history(path: &std::path::Path) -> Vec<Item> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|raw| item::parse(raw))
        .collect()
}

/// Prépare le guide (noms PoB + étapes), rejoue la session en cours, puis suit le log.
fn start_coach(ctx: egui::Context, config: &Config, files: Vec<PathBuf>, cache: PathBuf, state: PathBuf) -> (Receiver<coach::Coach>, Receiver<logwatch::Event>) {
    let (coach_tx, coach_rx) = channel();
    let (log_tx, log_rx) = channel();
    let log_path = config.log_path.clone();
    std::thread::spawn(move || {
        let catalog = guide::Catalog::load(&cache);
        let steps = guide::load_steps(&files, &catalog);
        let mut c = coach::Coach::new(steps, catalog, state);
        // Premier lancement : tout le log, pour retrouver les personnages, leur dernière zone
        // et les récompenses de la route déjà obtenues.
        let whole = !c.full_scan_done() || !c.route_scan_done();
        let events = if whole { logwatch::read_all(&log_path) } else { logwatch::read_recent(&log_path, 8 * 1024 * 1024) };
        for ev in events {
            c.handle(&ev);
        }
        c.mark_full_scan();
        c.mark_route_scan();
        c.live = true;
        c.save();
        let _ = coach_tx.send(c);
        ctx.request_repaint();
        logwatch::follow(log_path, log_tx, ctx);
    });
    (coach_rx, log_rx)
}

impl App {
    fn new(cc: &eframe::CreationContext, dirs: ProjectDirs, mut stored: Config, config: Config) -> App {
        // Nouveautés depuis la dernière version vue. Les installations d'avant ce suivi ont déjà
        // validé l'accueil : elles viennent au plus tard de la 0.2.3. Première installation : rien.
        let seen = match (&stored.seen_version, stored.welcomed) {
            (v, _) if !v.is_empty() => v.clone(),
            (_, true) => "0.2.3".to_string(),
            (_, false) => updater::VERSION.to_string(),
        };
        let news = changelog::since(&seen, updater::VERSION);
        if news.is_empty() && stored.seen_version != updater::VERSION {
            stored.seen_version = updater::VERSION.to_string();
            stored.save(&dirs);
        }
        style(&cc.egui_ctx);
        let (tx, rx) = channel();
        capture::spawn(tx, cc.egui_ctx.clone());
        let leagues = leagues::Leagues::start(&cc.egui_ctx, dirs.cache_dir().to_path_buf());
        let league = Some(config.league.clone()).filter(|l| !l.is_empty()).or_else(|| leagues.default_league()).unwrap_or_else(|| "Standard".into());
        let prices = prices::Prices::start(dirs.cache_dir().to_path_buf(), league.clone());
        let builds = builds::BuildStore::open(dirs.data_dir());
        let files = builds.active().map(|b| builds.files(&b.id)).unwrap_or_default();
        let (coach_rx, log_rx) =
            start_coach(cc.egui_ctx.clone(), &config, files, dirs.cache_dir().to_path_buf(), dirs.data_dir().join("state.json"));
        App {
            gear: Gear::load(&dirs.data_dir().join("gear.json")),
            assets: art::Assets::new(&cc.egui_ctx, dirs.cache_dir().to_path_buf(), league.clone()),
            ui_art: ui_art::UiArt::new(&cc.egui_ctx, dirs.cache_dir().to_path_buf(), config.game_dir.clone()),
            prices: prices.clone(),
            trade: trade::Trade::start(cc.egui_ctx.clone(), dirs.cache_dir().to_path_buf(), league.clone(), prices),
            augments: augments::Augments::start(dirs.cache_dir().to_path_buf()),
            gem_infos: gem_info::GemInfos::start(dirs.cache_dir().to_path_buf()),
            socketed: HashMap::new(),
            socket_tips: HashMap::new(),
            catalog_id: 0,
            build_runes: None,
            build_quests: None,
            step_preview: None,
            market_min_ex: config.market_min_ex,
            copies: rx,
            diagnosed: None,
            history: load_history(&dirs.data_dir().join("history.json")),
            history_path: dirs.data_dir().join("history.json"),
            open_diag: std::env::var("POE2_TRACKER_DIAG").ok().and_then(|v| v.parse().ok()),
            status: None,
            coach: None,
            coach_rx,
            log_rx,
            feed: VecDeque::new(),
            builds,
            // Développement : `POE2_TRACKER_BUILDS` ouvre le gestionnaire de builds au démarrage.
            add_build: AddBuild { open: std::env::var_os("POE2_TRACKER_BUILDS").is_some(), ..Default::default() },
            data_dir: dirs.data_dir().to_path_buf(),
            character: None,
            bar_h: 34.0,
            // Développement : onglet de droite (POE2_TRACKER_SIDE=market).
            market_category: "Currency".into(),
            market_search: String::new(),
            market_league: None,
            estimate_open: None,
            side_tab: if std::env::var("POE2_TRACKER_SIDE").as_deref() == Ok("market") {
                views::SideTab::Market
            } else {
                views::SideTab::Diagnostic
            },
            tab: match std::env::var("POE2_TRACKER_TAB").as_deref() {
                Ok("carac") => views::Tab::Stats,
                Ok("sorts") => views::Tab::Skills,
                Ok("talents") => views::Tab::Passives,
                Ok("ascendances") => views::Tab::Ascendancy,
                Ok("uniques") => views::Tab::Uniques,
                _ => views::Tab::Equipment,
            },
            view_step: None,
            tree: views::TreeView::default(),
            passive_icons: passive_icons::PassiveIcons::new(&cc.egui_ctx, dirs.cache_dir().to_path_buf(), config.game_dir.clone()),
            welcome: Welcome {
                // Premier lancement, ou jeu introuvable : on demande avant tout.
                open: !stored.welcomed || !paths::check(&config.game_dir).ok(),
                path: config.game_dir.display().to_string(),
                ..Default::default()
            },
            updater: updater::Updater::start(&cc.egui_ctx, config.auto_update),
            // Développement : réglages ouverts d'office (POE2_TRACKER_SETTINGS=1).
            settings: std::env::var_os("POE2_TRACKER_SETTINGS")
                .is_some()
                .then(|| SettingsDraft::new(&stored, &config)),
            changelog: (!news.is_empty()).then_some(news),
            dirs,
            stored,
            config,
            leagues,
            league,
        }
    }

    /// Ligue du personnage, sinon celle des réglages, sinon la première ligue en cours.
    fn effective_league(&self) -> String {
        self.coach
            .as_ref()
            .and_then(|c| c.league())
            .or_else(|| Some(self.config.league.clone()).filter(|l| !l.is_empty()))
            .or_else(|| self.leagues.default_league())
            .unwrap_or_else(|| "Standard".into())
    }

    /// Relance prix et estimations quand la ligue change (autre personnage, autre choix).
    fn sync_league(&mut self, ctx: &egui::Context) {
        let want = self.effective_league();
        if want == self.league {
            return;
        }
        let cache = self.dirs.cache_dir().to_path_buf();
        self.prices = prices::Prices::start(cache.clone(), want.clone());
        self.trade = trade::Trade::start(ctx.clone(), cache, want.clone(), self.prices.clone());
        self.assets.set_league(&want);
        self.league = want;
    }

    /// Nouveau dossier du jeu : images, icônes et lecture du journal repartent de là.
    fn set_game_dir(&mut self, ctx: &egui::Context, dir: PathBuf) {
        self.stored.game_dir = dir;
        self.stored.log_path = PathBuf::new();
        self.config = self.stored.resolved();
        let cache = self.dirs.cache_dir().to_path_buf();
        self.ui_art = ui_art::UiArt::new(ctx, cache.clone(), self.config.game_dir.clone());
        self.passive_icons = passive_icons::PassiveIcons::new(ctx, cache.clone(), self.config.game_dir.clone());
        let files = self.builds.active().map(|b| self.builds.files(&b.id)).unwrap_or_default();
        let (coach_rx, log_rx) = start_coach(ctx.clone(), &self.config, files, cache, self.data_dir.join("state.json"));
        (self.coach, self.coach_rx, self.log_rx) = (None, coach_rx, log_rx);
        self.feed.clear();
    }

    fn welcome_window(&mut self, ctx: &egui::Context) {
        if !self.welcome.open {
            return;
        }
        if let Some(Ok(picked)) = self.welcome.picking.as_ref().map(|rx| rx.try_recv()) {
            self.welcome.picking = None;
            if let Some(p) = picked {
                self.welcome.path = p.display().to_string();
            }
        }
        let dir = PathBuf::from(self.welcome.path.trim());
        let check = paths::check(&dir);
        let first = !self.stored.welcomed;
        let mut done = false;
        let mut cancel = false;
        let title = if first { "Bienvenue dans poe2-tracker" } else { "Dossier du jeu" };
        // Premier lancement : on ne ferme qu'avec « Commencer ».
        let (_, closed) = ui::modal(ctx, "welcome", title, !first, 600.0, |ui| {
                if first {
                    ui.label("poe2-tracker accompagne ta partie sans jamais toucher au jeu : il lit seulement son journal et ce que tu copies avec Ctrl+C.");
                    ui.add_space(8.0);
                }
                ui.label(RichText::new("Dossier d'installation de Path of Exile 2").color(ui::GOLD).strong());
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.welcome.path).desired_width(380.0).hint_text("dossier qui contient « logs »"));
                    if ui.add_enabled(self.welcome.picking.is_none(), egui::Button::new("Parcourir…")).clicked() {
                        let (tx, rx) = channel();
                        let ctx = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(rfd::FileDialog::new().set_title("Dossier de Path of Exile 2").pick_folder());
                            ctx.request_repaint();
                        });
                        self.welcome.picking = Some(rx);
                    }
                    if ui.button("Rechercher").clicked() {
                        match paths::detect() {
                            Some(d) => {
                                self.welcome.path = d.display().to_string();
                                self.welcome.message = None;
                            }
                            None => self.welcome.message = Some("Introuvable aux emplacements habituels (Steam, version autonome) : indique-le avec « Parcourir ».".into()),
                        }
                    }
                });
                let line = |ui: &mut egui::Ui, ok: bool, what: &str, missing: &str| {
                    let (mark, color) = if ok { ("✔", Color32::from_rgb(120, 200, 120)) } else { ("✘", Color32::from_rgb(230, 110, 90)) };
                    ui.label(RichText::new(format!("{mark}  {what}{}", if ok { String::new() } else { format!(" — {missing}") })).color(color));
                };
                line(ui, check.log, "Journal du jeu (logs/Client.txt)", "pas de personnage, de route de leveling ni de fil d'événements");
                line(ui, check.assets, "Images du jeu (Bundles2 ou Content.ggpk)", "pas de fond ni d'icônes de l'arbre");
                if !check.log && !check.assets {
                    ui.label(RichText::new("Jeu introuvable ici. Le diagnostic des objets (Ctrl+C) marche quand même.").color(Color32::from_rgb(230, 110, 90)));
                }
                if let Some(m) = &self.welcome.message {
                    ui.label(RichText::new(m).color(Color32::GRAY));
                }
                if first {
                    ui.add_space(10.0);
                    ui.label(RichText::new("En jeu").color(ui::GOLD).strong());
                    ui.label("•  Ctrl+C sur un objet : diagnostic (comparaison avec ta pièce, valeur au marché, verdict).");
                    ui.label("•  Ctrl+Alt+C sur une pièce que tu portes : elle rejoint l'équipement suivi, base des comparaisons.");
                    ui.label("•  En bas : la route de leveling (points de passif, résistances…), cochée toute seule au fil de la campagne.");
                    ui.add_space(10.0);
                    ui.label(RichText::new("Build (facultatif)").color(ui::GOLD).strong());
                    ui.label("Sans build, le diagnostic compare simplement à tes pièces. Avec un build (lien Kami-labs ou fichier .build du planificateur), les verdicts suivent ses objectifs et les onglets Sorts, Talents et Ascendances s'ouvrent.");
                    if ui.button("Ajouter un build…").clicked() {
                        self.add_build.open = true;
                        self.add_build.message = None;
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(if first { "Commencer" } else { "Valider" }).color(ui::GOLD).strong()).clicked() {
                            done = true;
                        }
                        if !first && ui.button("Annuler").clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if closed {
            cancel = true;
        }
        if cancel {
            self.welcome.open = false;
        }
        if done {
            self.welcome.open = false;
            if dir != self.config.game_dir {
                self.set_game_dir(ctx, dir);
            }
            self.stored.welcomed = true;
            self.stored.save(&self.dirs);
        }
    }

    /// Étape qui sert à juger l'équipement : celle de la progression, ou celle choisie en aperçu.
    fn judging_step(&self) -> Option<&guide::Step> {
        let c = self.coach.as_ref()?;
        self.step_preview.as_ref().and_then(|l| c.steps.iter().find(|s| &s.label == l)).or_else(|| c.judging_step())
    }

    /// Build actif et étape utilisée pour juger l'équipement.
    fn basis(&self) -> Option<(String, guide::Step)> {
        let name = self.builds.active()?.name.clone();
        let step = self.judging_step()?.clone();
        Some((name, step))
    }

    /// Augments posés dans une pièce, reconnus d'après ses effets (`None` sans châsse, ou tant
    /// que le catalogue n'est pas chargé).
    fn socketed(&mut self, it: &Item) -> Option<augments::Socketed> {
        if it.sockets == 0 && !it.mod_kinds.contains(&item::ModKind::Rune) {
            return None;
        }
        let catalog = self.augments.get()?;
        // Catalogue remplacé (mise à jour) : les reconnaissances et conseils sont à refaire.
        let id = Arc::as_ptr(&catalog) as usize;
        if id != self.catalog_id {
            self.catalog_id = id;
            self.socketed.clear();
            self.socket_tips.clear();
        }
        Some(self.socketed.entry(it.raw.clone()).or_insert_with(|| augments::identify(it, &catalog.augments)).clone())
    }

    /// Augments conseillés pour les châsses vides d'une pièce portée en `slot`, selon les
    /// objectifs du build à l'étape en cours, avec leur prix.
    fn socket_tips(&mut self, it: &Item, slot: Slot) -> Vec<String> {
        let (Some(catalog), Some((_, step))) = (self.augments.get(), self.basis()) else { return vec![] };
        let level = self.coach.as_ref().map(|c| c.level()).unwrap_or(100);
        let key = (it.raw.clone(), step.label.clone(), level);
        if let Some(t) = self.socket_tips.get(&key) {
            return t.clone();
        }
        let prices = self.prices.data.lock().unwrap();
        let tips: Vec<String> = augments::suggestions(it, &catalog.augments, level, |v| compare::build_value(&step, &[slot], v))
            .into_iter()
            .map(|(a, e)| {
                let price = prices.as_ref().and_then(|p| Some(format!(" ≈ {}", p.fmt(p.currency(&a.name)?)))).unwrap_or_default();
                format!("{} : {}{price}", a.name, e.text.join(", "))
            })
            .collect();
        drop(prices);
        self.socket_tips.insert(key, tips.clone());
        tips
    }

    /// Runes que le build recommande sur `slot` à l'étape en cours (page Kami-labs liée).
    fn recommended_runes(&mut self, slot: Slot) -> Vec<String> {
        let Some(id) = self.builds.active().map(|b| b.id.clone()) else { return vec![] };
        if self.build_runes.as_ref().is_none_or(|(b, _)| *b != id) {
            self.build_runes = Some((id.clone(), builds::load_runes(&self.builds.dir_of(&id))));
        }
        let Some(label) = self.judging_step().map(|s| s.label.clone()) else { return vec![] };
        let steps = &self.build_runes.as_ref().unwrap().1;
        let runes = steps
            .iter()
            .find(|s| s.name == label)
            .and_then(|s| s.items.iter().find(|(k, _)| builds::kami_slot(k) == Some(slot)))
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        // Mobalytics donne des identifiants (« soulcore-runespecial9 ») : on veut les noms.
        match self.augments.get() {
            Some(catalog) => runes.iter().map(|r| catalog.rune_name(r)).collect(),
            None => runes,
        }
    }

    /// Choix de récompenses de quête que le build recommande (Mobalytics).
    fn quest_choices(&mut self) -> Vec<builds::QuestChoice> {
        let Some(id) = self.builds.active().map(|b| b.id.clone()) else { return vec![] };
        if self.build_quests.as_ref().is_none_or(|(b, _)| *b != id) {
            self.build_quests = Some((id.clone(), builds::load_quest_choices(&self.builds.dir_of(&id))));
        }
        self.build_quests.as_ref().unwrap().1.clone()
    }

    /// « Greater Iron Rune ×2 ≈ 3 ex » : runes regroupées, avec leur prix.
    fn rune_list(&self, runes: &[String]) -> Vec<String> {
        let prices = self.prices.data.lock().unwrap();
        let mut out: Vec<(String, usize)> = Vec::new();
        for r in runes {
            match out.iter_mut().find(|(n, _)| n == r) {
                Some((_, c)) => *c += 1,
                None => out.push((r.clone(), 1)),
            }
        }
        out.into_iter()
            .map(|(n, c)| {
                let price = prices.as_ref().and_then(|p| Some(format!(" ≈ {}", p.fmt(p.currency(&n)?)))).unwrap_or_default();
                if c > 1 { format!("{n} ×{c}{price}") } else { format!("{n}{price}") }
            })
            .collect()
    }

    /// Colonne « recommandé par le build » de la fiche d'une pièce portée en `slot`.
    /// `it` : la pièce portée, comparée ligne par ligne ; `None` pour une case vide.
    fn build_card(&mut self, it: Option<&Item>, slot: Slot) -> Option<ui::BuildCard> {
        let (_, step) = self.basis()?;
        let base = step.targets.iter().find(|(s, _, _)| *s == slot).map(|(_, b, _)| b.clone()).unwrap_or_default();
        let unique = self.recommended_unique(&step, slot);
        let catalog = self.augments.get();
        let lines = match it {
            Some(it) => compare::check_targets(it, &step, slot, |b| catalog.as_ref()?.defences.get(b).copied()),
            None => step.targets_for(slot).map(|t| (t.line.clone(), compare::Have::Unknown)).collect(),
        };
        let runes = self.recommended_runes(slot);
        let base_defences = catalog.as_ref().and_then(|c| c.defences.get(&base).copied());
        let implicits = catalog.as_ref().and_then(|c| c.implicits.get(&base).cloned()).unwrap_or_default();
        let image = match &unique {
            Some(u) => self.assets.texture_unique(u),
            None if !base.is_empty() => self.assets.texture_by_name(&base),
            None => None,
        };
        let card = ui::BuildCard { step: step.label.clone(), base, base_defences, implicits, unique, lines, runes: self.rune_list(&runes),
            image,
        };
        (!card.is_empty()).then_some(card)
    }

    /// Unique que le build recommande sur `slot` à cette étape (une fiole de mana est rangée par
    /// le planificateur avec les fioles de vie : son type de base la remet à sa place).
    fn recommended_unique(&self, step: &guide::Step, slot: Slot) -> Option<String> {
        let prices = self.prices.data.lock().unwrap();
        let base = |n: &str| prices.as_ref()?.unique_by_name(n).map(|(b, _, _)| b.to_string());
        step.uniques.iter().find(|w| uniques::slot(w.slot, base(&w.name).as_deref()) == Some(slot)).map(|w| w.name.clone())
    }

    /// Pièce recommandée sur une case vide, en noir et blanc : l'unique, sinon le type de base.
    fn recommended_ghost(&mut self, slot: Slot) -> Option<egui::TextureHandle> {
        let (_, step) = self.basis()?;
        if let Some(name) = self.recommended_unique(&step, slot) {
            return self.assets.texture_unique_gray(&name);
        }
        let base = step.targets.iter().find(|(s, _, _)| *s == slot).map(|(_, b, _)| b.clone()).filter(|b| !b.is_empty())?;
        self.assets.texture_by_name_gray(&base)
    }

    /// Vue des châsses d'une pièce suivie pour la grille d'équipement.
    fn socket_view(&mut self, it: &Item, slot: Slot) -> Option<ui::SocketView> {
        let s = self.socketed(it)?;
        let wanted = self.recommended_runes(slot);
        // Chaque augment posé est-il un de ceux que le build conseille ici ? Ce qui reste des
        // conseils s'affiche en noir et blanc dans les châsses vides.
        let mut missing = wanted.clone();
        let filled = s
            .augments
            .iter()
            .map(|n| {
                let good = (!wanted.is_empty()).then(|| match missing.iter().position(|m| m == n) {
                    Some(i) => {
                        missing.remove(i);
                        true
                    }
                    None => false,
                });
                (n.clone(), self.assets.texture_by_name(n), good)
            })
            .collect();
        let ghosts = missing.iter().take(s.empty()).map(|n| self.assets.texture_by_name_gray(n)).collect();
        let suggestions = if s.empty() > 0 && wanted.is_empty() { self.socket_tips(it, slot) } else { vec![] };
        Some(ui::SocketView { filled, empty: s.empty(), unknown: s.unknown, ghosts, suggestions })
    }

    fn compare(&self, it: &Item) -> Option<compare::Comparison> {
        let basis = self.basis();
        let catalog = self.augments.get();
        let base = self.assets.index.lock().unwrap().as_ref().and_then(|i| i.base_of(it)).unwrap_or_else(|| it.base.clone());
        let item_base = catalog.as_ref().and_then(|c| c.base(&base));
        compare::compare(it, &self.gear, basis.as_ref().map(|(n, s)| (n.as_str(), s)), item_base, &|n| catalog.as_ref()?.base(n))
    }

    /// Active un build et recharge ses étapes dans le guide.
    fn activate_build(&mut self, id: &str) {
        self.builds.set_active(id);
        self.step_preview = None;
        let files = self.builds.files(id);
        if let Some(c) = &mut self.coach {
            c.reload_steps(&files);
        }
        if let Some(b) = self.builds.active() {
            let name = b.name.clone();
            self.say(format!("Build actif : {name}"));
        }
    }

    fn import_files(&mut self, files: Vec<builds::BuildFile>, source: &str) {
        match self.builds.add(files, source) {
            Ok(meta) => {
                let id = meta.id.clone();
                self.activate_build(&id);
                self.add_build.message = Some((true, format!("« {} » ajouté et activé", meta.name)));
                self.add_build.url.clear();
            }
            Err(e) => self.add_build.message = Some((false, e)),
        }
    }

    /// Barre du haut : build actif, ajout et retrait.
    fn build_bar(&mut self, ui: &mut egui::Ui) {
        // Toute la largeur du bandeau, dans un cadre sombre bordé de doré assorti au fond du jeu.
        let width = ui.available_width();
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(8, 7, 6, 215))
            .stroke(egui::Stroke::new(1.0_f32, ui::GOLD_DIM))
            .corner_radius(4)
            // Un pixel de moins en haut : les listes, centrées sur la hauteur de la ligne, tombaient
            // un peu bas dans le cadre.
            .inner_margin(egui::Margin {
                left: 10,
                right: 10,
                top: 3,
                bottom: 5,
            })
            .show(ui, |ui| {
                ui.set_width(width - 22.0);
                ui::gold_widgets(ui);
                ui.horizontal_centered(|ui| self.build_bar_content(ui));
            });
    }

    fn build_bar_content(&mut self, ui: &mut egui::Ui) {
        let chars = self.coach.as_ref().map(|c| c.characters()).unwrap_or_default();
        let current_char = self.coach.as_ref().and_then(|c| c.character()).map(|(n, _)| n.to_string());
        let confirmed = self.coach.as_ref().is_some_and(|c| c.confirmed);
        let has_character = current_char.is_some();
        // Étape du build : toutes, campagne comprise. Le journal du jeu suit l'acte, mais pas la
        // variante d'endgame jouée : en endgame, la choisir l'enregistre. Toute autre étape
        // s'affiche en aperçu (verdicts, étoiles, recommandations), sans rien enregistrer.
        let all_steps: Vec<String> = self.coach.as_ref().map(|c| c.steps.iter().map(|s| s.label.clone()).collect()).unwrap_or_default();
        let progress = self.coach.as_ref().and_then(|c| c.judging_step()).map(|s| s.label.clone());
        let in_endgame = self.coach.as_ref().and_then(|c| c.character()).is_some_and(|(_, s)| s.max_act > 5);
        let shown_step = self.step_preview.clone().or(progress.clone());

        // Les listes présentes se partagent toute la largeur de la barre, chacune selon sa part.
        // Une liste ne se coupe qu'à la largeur de sa zone (sa propre largeur n'est qu'un
        // minimum) : chacune est posée dans une zone de largeur fixe, sa part.
        let shown = [!chars.is_empty(), true, has_character, shown_step.is_some()];
        let weights = [0.22, 0.42, 0.14, 0.22];
        let count = shown.iter().filter(|s| **s).count() as f32;
        let total_weight: f32 = shown.iter().zip(weights).filter(|(s, _)| **s).map(|(_, w)| w).sum();
        // Place gardée tout à droite pour le lien vers la page du build.
        let link_w = 30.0;
        let room = ui.available_width() - ui.spacing().item_spacing.x * count - link_w;
        let zone = |i: usize| room * weights[i] / total_weight;
        let width = zone;
        let height = ui.available_height();
        let in_zone = |ui: &mut egui::Ui, i: usize, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.allocate_ui_with_layout(Vec2::new(zone(i), height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(zone(i));
                add(ui);
            });
        };

        let mut picked: Option<String> = None;
        if shown[0] {
            in_zone(ui, 0, &mut |ui| {
            let label = match (&current_char, confirmed) {
                (Some(n), true) => RichText::new(n.clone()).color(ui::GOLD),
                (Some(n), false) => RichText::new(format!("{n} ?")).color(ui::GOLD).italics(),
                (None, _) => RichText::new("personnage ?").color(Color32::GRAY),
            };
            let resp = egui::ComboBox::from_id_salt("character_select").width(width(0)).truncate().selected_text(label).show_ui(ui, |ui| {
                for (name, class, level) in &chars {
                    if ui.selectable_label(current_char.as_ref() == Some(name), format!("{name} — {class} niv. {level}")).clicked() {
                        picked = Some(name.clone());
                    }
                }
            });
            if !confirmed && has_character {
                resp.response.on_hover_text("Personnage supposé d'après le journal du jeu : à confirmer");
            }
            });
        }
        if let Some(name) = picked {
            if let Some(c) = &mut self.coach {
                c.set_character(&name);
            }
            self.switch_character(Some(name));
        }

        let current = self.builds.active().map(|b| b.name.clone());
        let selected = match &current {
            Some(n) => RichText::new(n).color(ui::GOLD),
            None => RichText::new("aucun build").color(Color32::GRAY),
        };
        let mut chosen: Option<String> = None;
        // Cadre de la liste des builds : le bouton de lien prend exactement sa hauteur.
        let mut combo_rect: Option<egui::Rect> = None;
        in_zone(ui, 1, &mut |ui| {
            let combo = egui::ComboBox::from_id_salt("build_select").width(width(1)).truncate().selected_text(selected.clone()).show_ui(ui, |ui| {
            for b in self.builds.list() {
                let active = self.builds.active().is_some_and(|a| a.id == b.id);
                if ui.selectable_label(active, &b.name).on_hover_text(format!("source : {}", b.source)).clicked() {
                    chosen = Some(b.id.clone());
                }
            }
        })
        .response
        .on_hover_text(current.as_deref().unwrap_or("aucun build"));
            combo_rect = Some(combo.rect);
        });

        let mut league_pick: Option<String> = None;
        if shown[2] {
            in_zone(ui, 2, &mut |ui| {
            let mut list = self.leagues.list();
            if !list.contains(&self.league) {
                list.insert(0, self.league.clone());
            }
            egui::ComboBox::from_id_salt("league_select")
                .truncate()
                .width(width(2))
                .selected_text(RichText::new(&self.league).color(ui::GOLD_DIM))
                .show_ui(ui, |ui| {
                    for l in list {
                        if ui.selectable_label(l == self.league, &l).clicked() {
                            league_pick = Some(l);
                        }
                    }
                })
                .response
                .on_hover_text("Ligue de ce personnage, pour les prix du marché (le journal du jeu ne la donne pas)");
            });
        }

        let mut step_pick: Option<String> = None;
        if let Some(shown_label) = &shown_step {
            in_zone(ui, 3, &mut |ui| {
            let preview = self.step_preview.is_some();
            let mut text = RichText::new(if preview { format!("{shown_label} (aperçu)") } else { shown_label.clone() }).color(ui::GOLD_DIM);
            if preview {
                text = text.italics();
            }
            egui::ComboBox::from_id_salt("step_select")
                .truncate()
                .width(width(3))
                .selected_text(text)
                .show_ui(ui, |ui| {
                    for label in &all_steps {
                        let current = progress.as_ref() == Some(label);
                        let name = if current { format!("{label} (actuelle)") } else { label.clone() };
                        if ui.selectable_label(Some(label) == shown_step.as_ref(), name).clicked() {
                            step_pick = Some(label.clone());
                        }
                    }
                })
                .response
                .on_hover_text(if preview {
                    "Étape vue en aperçu : reviens sur l'étape « (actuelle) » pour suivre ta progression"
                } else {
                    "Étape du build : en endgame, choisis la variante que tu joues ; les autres s'affichent en aperçu"
                });
            });
        }
        // Lien sortant : la page du build, ouverte sur l'étape affichée si le site le permet.
        let link = self.builds.active().map(|b| b.id.clone()).and_then(|id| {
            self.builds
                .step_link(&id, shown_step.as_deref().unwrap_or(""))
        });
        if let Some(url) = link {
            let hint = match &shown_step {
                Some(step) => format!("Ouvrir le build sur son site, à l'étape {step}"),
                None => "Ouvrir le build sur son site".to_string(),
            };
            // Même hauteur et même ligne que les listes.
            let (zone, _) = ui.allocate_exact_size(Vec2::new(link_w, height), egui::Sense::hover());
            let rect = match combo_rect {
                Some(c) => egui::Rect::from_min_max(
                    egui::pos2(zone.left(), c.top()),
                    egui::pos2(zone.right(), c.bottom()),
                ),
                None => zone,
            };
            let clicked = ui
                .put(rect, egui::Button::new(RichText::new("↗").color(ui::GOLD)))
                .on_hover_text(hint)
                .clicked();
            if clicked {
                if let Err(e) = desktop::open_url(&url) {
                    self.say(format!("Impossible d'ouvrir le navigateur : {e}"));
                }
            }
        }
        if let Some(id) = chosen {
            self.activate_build(&id);
        }
        if let (Some(l), Some(c)) = (league_pick, &mut self.coach) {
            c.set_league(l);
        }
        if let Some(label) = step_pick {
            let endgame_index = self.coach.as_ref().and_then(|c| guide::endgame_steps(&c.steps).iter().position(|s| s.label == label));
            match (in_endgame, endgame_index) {
                // Variante d'endgame jouée : enregistrée.
                (true, Some(i)) => {
                    if let Some(c) = &mut self.coach {
                        c.set_endgame_step(i as u32 + 1);
                    }
                    self.step_preview = None;
                }
                _ if Some(&label) == progress.as_ref() => self.step_preview = None,
                _ => self.step_preview = Some(label),
            }
        }
    }

    /// Nouvelle version publiée : bouton pour l'installer, puis pour relancer.
    fn update_status(&mut self, ui: &mut egui::Ui) {
        use updater::State;
        ui::gold_widgets(ui);
        match self.updater.state() {
            State::UpToDate => {
                ui.label(RichText::new(format!("v{}", updater::VERSION)).color(Color32::from_gray(90)).small());
            }
            State::Available { version, .. } => {
                let button = egui::Button::new(RichText::new(format!("⬆  Version {version} disponible — mettre à jour")).color(Color32::from_rgb(120, 200, 120)));
                if ui.add(button).on_hover_text(format!("Tu as la version {}", updater::VERSION)).clicked() {
                    self.updater.install(ui.ctx());
                }
            }
            State::Downloading { version } => {
                ui.label(RichText::new(format!("téléchargement de la version {version}…")).color(Color32::GRAY));
                ui.spinner();
            }
            State::Ready { version } => {
                if ui.button(RichText::new(format!("Version {version} installée — redémarrer")).color(Color32::from_rgb(120, 200, 120))).clicked() {
                    match updater::restart() {
                        Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.say(format!("Redémarrage impossible : {e} (relance l'app à la main)")),
                    }
                }
            }
            State::Failed(e) => {
                ui.label(RichText::new("mise à jour impossible").color(Color32::from_rgb(230, 120, 100))).on_hover_text(e);
            }
        }
    }

    /// Copie dans le presse-papier tout l'équipement suivi, avec son contexte (personnage,
    /// ligue, build), en Markdown.
    fn export_gear(&mut self, ctx: &egui::Context) {
        let count = self.gear.slots.len();
        let character = self.coach.as_ref().and_then(|c| c.character().map(|(n, s)| (n.to_string(), s.class.clone(), s.level)));
        let title = match &character {
            Some((name, ..)) => format!("Équipement de {name} (Path of Exile 2)"),
            None => "Équipement (Path of Exile 2)".to_string(),
        };
        let mut context = Vec::new();
        if let Some((_, class, level)) = &character {
            context.push(format!("Classe : {class}, niveau {level}"));
        }
        if !self.league.is_empty() {
            context.push(format!("Ligue : {}", self.league));
        }
        if let Some(b) = self.builds.active() {
            let step = self.judging_step().map(|s| format!(" (étape : {})", s.label));
            context.push(format!("Build suivi : {}{}", b.name, step.unwrap_or_default()));
        }
        context.push("Texte des pièces tel que copié en jeu (Ctrl+Alt+C), en anglais".into());
        ctx.copy_text(self.gear.export(&title, &context));
        self.say(format!("Équipement copié dans le presse-papier ({count} pièce(s))"));
    }

    /// Nouveautés : après une mise à jour (versions arrivées depuis la dernière vue), ou depuis
    /// le menu (toutes).
    /// Réglages de l'application : appliqués et enregistrés par « Enregistrer ».
    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.settings else {
            return;
        };
        if let Some(Ok(picked)) = draft.picking.as_ref().map(|rx| rx.try_recv()) {
            draft.picking = None;
            if let Some(p) = picked {
                draft.game_dir = p;
            }
        }
        let (mut save, mut cancel) = (false, false);
        let (_, closed) = ui::modal(ctx, "settings", "Réglages", true, 620.0, |ui| {
            // Tableau : intitulé (et son explication en petit) à gauche, réglage à droite.
            let label = |ui: &mut egui::Ui, title: &str, hint: &str| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    ui.label(RichText::new(title).color(Color32::from_gray(225)));
                    if !hint.is_empty() {
                        ui.label(RichText::new(hint).color(Color32::GRAY).small());
                    }
                });
            };
            let section = |ui: &mut egui::Ui, title: &str| {
                ui.label(RichText::new(title).color(ui::GOLD).strong());
                ui.end_row();
            };
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    section(ui, "Jeu");
                    let check = paths::check(&draft.game_dir);
                    let found = match (check.log, check.assets) {
                        (true, true) => "journal et images du jeu trouvés",
                        (true, false) => "journal trouvé, images du jeu introuvables",
                        (false, true) => "images trouvées, journal du jeu introuvable",
                        (false, false) => "jeu introuvable dans ce dossier",
                    };
                    label(ui, "Dossier du jeu", found);
                    // Le chemin est le bouton : un clic ouvre le choix du dossier.
                    let dir = draft.game_dir.display().to_string();
                    let shown = if dir.is_empty() {
                        "choisir le dossier…".to_string()
                    } else {
                        dir.clone()
                    };
                    let button =
                        egui::Button::new(RichText::new(format!("🗁  {shown}")).color(ui::GOLD))
                            .truncate()
                            .min_size(Vec2::new(320.0, 0.0));
                    let resp = ui
                        .add_enabled(draft.picking.is_none(), button)
                        .on_hover_text(if dir.is_empty() {
                            "Choisir le dossier du jeu".to_string()
                        } else {
                            format!("{dir}\nclic : choisir un autre dossier")
                        });
                    if resp.clicked() {
                        let (tx, rx) = channel();
                        let ctx = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(
                                rfd::FileDialog::new()
                                    .set_title("Dossier de Path of Exile 2")
                                    .pick_folder(),
                            );
                            ctx.request_repaint();
                        });
                        draft.picking = Some(rx);
                    }
                    ui.end_row();

                    section(ui, "Diagnostic (Ctrl+C)");
                    label(ui, "Affichage", "ce que fait Ctrl+C sur un objet en jeu");
                    egui::ComboBox::from_id_salt("open_on_copy")
                        .width(320.0)
                        .selected_text(
                            RichText::new(if draft.open_on_copy {
                                "ouvrir le détail du diagnostic"
                            } else {
                                "rester sur l'historique"
                            })
                            .color(ui::GOLD),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut draft.open_on_copy,
                                true,
                                "ouvrir le détail du diagnostic",
                            );
                            ui.selectable_value(
                                &mut draft.open_on_copy,
                                false,
                                "rester sur l'historique",
                            );
                        });
                    ui.end_row();

                    section(ui, "Prix");
                    label(
                        ui,
                        "Estimation sur le trade",
                        "recherche d'annonces comparables sur pathofexile.com",
                    );
                    ui.checkbox(&mut draft.trade_prices, "activée");
                    ui.end_row();
                    label(
                        ui,
                        "Lancement de l'estimation",
                        "au Ctrl+C, ou plus tard avec le bouton « Estimer »",
                    );
                    ui.add_enabled_ui(draft.trade_prices, |ui| {
                        egui::ComboBox::from_id_salt("auto_estimate")
                            .width(320.0)
                            .selected_text(
                                RichText::new(if draft.auto_estimate {
                                    "automatique, dès le Ctrl+C"
                                } else {
                                    "manuel, bouton « Estimer »"
                                })
                                .color(ui::GOLD),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut draft.auto_estimate,
                                    true,
                                    "automatique, dès le Ctrl+C",
                                );
                                ui.selectable_value(
                                    &mut draft.auto_estimate,
                                    false,
                                    "manuel, bouton « Estimer »",
                                );
                            });
                    });
                    ui.end_row();
                    label(
                        ui,
                        "Seuil du marché",
                        "en dessous, un objet est conseillé au PNJ",
                    );
                    ui.add(
                        egui::DragValue::new(&mut draft.market_min_ex)
                            .range(1.0..=10_000.0)
                            .speed(1.0)
                            .suffix(" ex"),
                    );
                    ui.end_row();

                    section(ui, "Mises à jour");
                    label(
                        ui,
                        "Recherche automatique",
                        "sur GitHub, au lancement puis toutes les 5 minutes",
                    );
                    ui.checkbox(&mut draft.auto_update, "activée");
                    ui.end_row();
                });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui::action_button(ui, "✔  Enregistrer").clicked();
                    cancel = ui::action_button(ui, "Annuler").clicked();
                });
            });
        });
        if save {
            let draft = self.settings.take().unwrap();
            self.stored.trade_prices = draft.trade_prices;
            self.stored.auto_estimate = draft.auto_estimate;
            self.stored.open_on_copy = draft.open_on_copy;
            self.config.auto_estimate = draft.auto_estimate;
            self.config.open_on_copy = draft.open_on_copy;
            self.stored.auto_update = draft.auto_update;
            self.stored.market_min_ex = draft.market_min_ex;
            self.market_min_ex = draft.market_min_ex;
            self.config.trade_prices = draft.trade_prices;
            self.config.auto_update = draft.auto_update;
            self.config.market_min_ex = draft.market_min_ex;
            self.updater.set_enabled(draft.auto_update);
            if draft.game_dir != self.config.game_dir {
                self.set_game_dir(ctx, draft.game_dir);
            }
            self.stored.save(&self.dirs);
            self.say("Réglages enregistrés".into());
        } else if cancel || closed {
            self.settings = None;
        }
    }

    fn changelog_window(&mut self, ctx: &egui::Context) {
        let Some(releases) = &self.changelog else { return };
        let (ok, closed) = ui::modal(ctx, "changelog", "Nouveautés", true, 560.0, |ui| {
            ui::plain_widgets(ui);
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                for (i, r) in releases.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(10.0);
                    }
                    let current = r.version == updater::VERSION;
                    let title = if current { format!("Version {} (installée)", r.version) } else { format!("Version {}", r.version) };
                    ui.label(RichText::new(title).color(ui::GOLD).strong());
                    for item in &r.items {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("•").color(ui::GOLD_DIM));
                            ui.label(RichText::new(item).color(Color32::LIGHT_GRAY));
                        });
                    }
                }
            });
            ui.add_space(10.0);
            ui.vertical_centered(|ui| ui.button("  OK  ").clicked()).inner
        });
        if ok || closed {
            self.changelog = None;
            if self.stored.seen_version != updater::VERSION {
                self.stored.seen_version = updater::VERSION.to_string();
                self.stored.save(&self.dirs);
            }
        }
    }

    /// Copie la regex des marchands pour l'étape suivie : collée dans la recherche d'un
    /// marchand en jeu, elle surligne les objets qui portent un mod visé par le build.
    fn vendor_regex_button(&mut self, ui: &mut egui::Ui) {
        ui::gold_widgets(ui);
        let step = self.judging_step().cloned();
        let (regex, names) = step.as_ref().map(vendor_regex::build).unwrap_or_default();
        let button = ui.add_enabled(!regex.is_empty(), egui::Button::new(RichText::new("🔍  Regex marchands").color(ui::GOLD)));
        let button = button
            .on_hover_ui(|ui| {
                ui.set_max_width(420.0);
                let step = step.as_ref().map(|s| s.label.as_str()).unwrap_or_default();
                ui.label(RichText::new(format!("Copie une recherche pour les marchands, d'après le build ({step}) : colle-la (Ctrl+V) dans la recherche d'un marchand en jeu, les objets utiles s'allument.")).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(format!("Cherche : {}", names.join(", "))).size(15.0).color(Color32::GRAY));
            })
            .on_disabled_hover_text("Aucun build suivi, ou il ne vise aucun mod à cette étape");
        if button.clicked() {
            ui.ctx().copy_text(regex.clone());
            self.say(format!("Regex copiée ({} caractères) : colle-la dans la recherche d'un marchand", regex.chars().count()));
        }
    }

    /// Bouton « Menu » de la barre d'état, ouvert vers le haut : actions, puis réglages.
    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        ui::gold_widgets(ui);
        let button = ui.button(RichText::new("⚙  Menu").color(ui::GOLD));
        let popup = ui.make_persistent_id("settings_popup");
        if button.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        // Développement : menu ouvert d'office (POE2_TRACKER_MENU=1), pour le voir sans cliquer.
        if std::env::var_os("POE2_TRACKER_MENU").is_some() {
            ui.memory_mut(|m| m.open_popup(popup));
        }
        egui::popup::popup_above_or_below_widget(ui, popup, &button, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_width(280.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui::menu_section(ui, "BUILD ET JEU");
            if ui::menu_item(ui, "⚔", "Gestionnaire de builds", "ajouter, mettre à jour, choisir le build suivi", true, false).clicked() {
                self.add_build.open = true;
                self.add_build.message = None;
            }
            if ui::menu_item(ui,
                    "🔧",
                    "Réglages",
                    "dossier du jeu, prix, mises à jour",
                    true,
                    false,
                )
                .clicked()
                {
                    self.settings = Some(SettingsDraft::new(&self.stored, &self.config));
            }
            ui.add_space(4.0);
            ui.separator();
            ui::menu_section(ui, "DONNÉES");
            let tracked = !self.gear.slots.is_empty();
            let export_hint = if tracked { "copie le texte de chaque pièce suivie" } else { "aucune pièce suivie : Ctrl+Alt+C sur une pièce" };
            if ui::menu_item(ui, "📤", "Exporter l'équipement", export_hint, tracked, false).clicked() {
                self.export_gear(ui.ctx());
            }
            if ui::menu_item(ui, "⟳", "Mettre à jour les catalogues", "augments, types de base, gemmes, images", true, false).clicked() {
                self.augments.refresh();
                self.gem_infos.refresh();
                self.assets.refresh();
                self.say("Mise à jour des catalogues en cours (quelques secondes)".into());
            }
            if ui::menu_item(ui, "✨", "Nouveautés", &format!("ce qui a changé jusqu'à la version {}", updater::VERSION), true, false).clicked() {
                self.changelog = Some(changelog::releases());
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);
            if ui::menu_item(ui, "🗑", "Vider l'historique", "", !self.history.is_empty(), true).clicked() {
                self.history.clear();
                self.save_history();
                self.say("Historique vidé".into());
            }
            ui.add_space(2.0);
        });
    }

    /// Gestionnaire de builds : ajout par le lien de la page du build (Mobalytics, Kami-labs),
    /// mise à jour depuis cette page, activation, suppression.
    fn add_build_window(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.add_build.pending {
            if let Ok(result) = rx.try_recv() {
                self.add_build.pending = None;
                match result {
                    Ok(files) => {
                        let url = self.add_build.url.clone();
                        self.import_files(files, &url);
                    }
                    Err(e) => self.add_build.message = Some((false, e)),
                }
            }
        }
        if let Some((id, url, rx)) = &self.add_build.link_pending {
            if let Ok(result) = rx.try_recv() {
                let (id, url) = (id.clone(), url.clone());
                self.add_build.link_pending = None;
                let updated = result.and_then(|files| self.builds.update(&id, files, &url));
                self.add_build.message = Some(match updated {
                    Ok(meta) => {
                        self.add_build.link_for = None;
                        self.build_runes = None;
                        self.build_quests = None;
                        // Build en cours d'utilisation : ses nouvelles étapes tout de suite.
                        if self.builds.active().is_some_and(|b| b.id == id) {
                            let files = self.builds.files(&id);
                            if let Some(c) = &mut self.coach {
                                c.reload_steps(&files);
                            }
                        }
                        (true, format!("« {} » mis à jour", meta.name))
                    }
                    Err(e) => (false, e),
                });
            }
        }
        if !self.add_build.open {
            return;
        }
        let mut activate = None;
        let mut shift: Option<(String, isize)> = None;
        let mut link: Option<(String, String)> = None;
        let mut remove = None;
        let (_, closed) = ui::modal(ctx, "build_manager", "Gestionnaire de builds", true, 680.0, |ui| {
                // Liste des builds : une carte par build, actions à droite.
                if self.builds.list().is_empty() {
                    ui.label(RichText::new("Aucun build pour l'instant.").color(Color32::GRAY));
                }
                let active_id = self.builds.active().map(|b| b.id.clone());
                let count = self.builds.list().len();
                for (i, b) in self.builds.list().iter().enumerate() {
                    let active = active_id.as_ref() == Some(&b.id);
                    let web = b.source.starts_with("http");
                    egui::Frame::new()
                        .fill(Color32::from_rgb(20, 18, 15))
                        .stroke(egui::Stroke::new(1.0_f32, if active { ui::GOLD_DIM } else { Color32::from_rgb(50, 45, 38) }))
                        .corner_radius(4)
                        .inner_margin(10)
                        .outer_margin(egui::Margin::symmetric(0, 3))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                // Ordre de la liste (aussi celui de la liste des builds en haut).
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    if ui.add_enabled(i > 0, egui::Button::new(RichText::new("▲").small()).small()).on_hover_text("Monter").clicked() {
                                        shift = Some((b.id.clone(), -1));
                                    }
                                    if ui.add_enabled(i + 1 < count, egui::Button::new(RichText::new("▼").small()).small()).on_hover_text("Descendre").clicked() {
                                        shift = Some((b.id.clone(), 1));
                                    }
                                });
                                // Place réservée aux boutons, à droite : le texte ne passe jamais dessous.
                                let text_w = (ui.available_width() - 260.0).max(200.0);
                                ui.vertical(|ui| {
                                    ui.set_width(text_w);
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Label::new(RichText::new(&b.name).color(ui::GOLD).strong()).truncate()).on_hover_text(&b.name);
                                        if active {
                                            ui.label(RichText::new("● actif").color(Color32::from_rgb(110, 210, 110)).small());
                                        }
                                    });
                                    if web {
                                        ui.hyperlink_to(RichText::new(short_source(&b.source)).small(), &b.source).on_hover_text(&b.source);
                                    } else {
                                        ui.label(RichText::new(format!("{} · sans lien", b.source)).color(Color32::GRAY).small());
                                    }
                                    let busy = self.add_build.link_pending.as_ref().is_some_and(|(id, _, _)| *id == b.id);
                                    if busy {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label(RichText::new("mise à jour…").color(Color32::GRAY).small());
                                        });
                                    } else if self.add_build.link_for.as_ref() == Some(&b.id) {
                                        ui.horizontal(|ui| {
                                            ui.add(egui::TextEdit::singleline(&mut self.add_build.link_url).hint_text("lien du build sur Mobalytics ou Kami-labs").desired_width(text_w - 60.0));
                                            if ui.add_enabled(!self.add_build.link_url.trim().is_empty(), egui::Button::new("Lier")).clicked() {
                                                link = Some((b.id.clone(), self.add_build.link_url.trim().to_string()));
                                            }
                                        });
                                    } else if !web && ui::text_button(ui, "lier sa page web (Mobalytics, Kami-labs)").clicked() {
                                        self.add_build.link_for = Some(b.id.clone());
                                        self.add_build.link_url.clear();
                                    }
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if self.add_build.confirm_remove.as_ref() == Some(&b.id) {
                                        if ui.button("Annuler").clicked() {
                                            self.add_build.confirm_remove = None;
                                        }
                                        if ui.button(RichText::new("Confirmer").color(Color32::from_rgb(230, 100, 90))).clicked() {
                                            remove = Some(b.id.clone());
                                        }
                                    } else {
                                        if ui.button(RichText::new("Supprimer").color(Color32::from_rgb(220, 120, 110))).clicked() {
                                            self.add_build.confirm_remove = Some(b.id.clone());
                                        }
                                        // De droite à gauche : Activer, Mettre à jour, Supprimer à l'écran.
                                        if web
                                            && ui
                                                .add_enabled(self.add_build.link_pending.is_none(), egui::Button::new("Mettre à jour"))
                                                .on_hover_text("Retélécharge le build depuis sa page (étapes, objets, runes)")
                                                .clicked()
                                        {
                                            link = Some((b.id.clone(), b.source.clone()));
                                        }
                                        if !active && ui.button("Activer").clicked() {
                                            activate = Some(b.id.clone());
                                        }
                                    }
                                });
                            });
                        });
                }

                ui.add_space(12.0);
                ui::section_header(ui, "AJOUTER UN BUILD", None, |_| {});
                // Sites pris en charge, en couleur.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("Lien de la page du build, sur ");
                    ui.label(RichText::new("Mobalytics").color(Color32::from_rgb(90, 200, 190)).strong());
                    ui.label(" ou ");
                    ui.label(RichText::new("Kami-labs").color(Color32::from_rgb(240, 160, 70)).strong());
                    ui.label(" :");
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.add_build.url).hint_text("https://mobalytics.gg/poe-2/…").desired_width(ui.available_width() - 90.0));
                    let busy = self.add_build.pending.is_some();
                    if ui.add_enabled(!busy && !self.add_build.url.trim().is_empty(), egui::Button::new("Importer")).clicked() {
                        let (tx, rx) = channel();
                        let (url, ctx) = (self.add_build.url.clone(), ctx.clone());
                        std::thread::spawn(move || {
                            let _ = tx.send(builds::fetch_url(&url));
                            ctx.request_repaint();
                        });
                        self.add_build.pending = Some(rx);
                        self.add_build.message = None;
                    }
                    if busy {
                        ui.spinner();
                    }
                });
                if let Some((ok, msg)) = &self.add_build.message {
                    let color = if *ok { Color32::from_rgb(110, 210, 110) } else { Color32::from_rgb(230, 120, 100) };
                    ui.add_space(6.0);
                    ui.label(RichText::new(msg).color(color));
                }
            });
        if let Some((id, url)) = link {
            let (tx, rx) = channel();
            let (ctx, page) = (ctx.clone(), url.clone());
            std::thread::spawn(move || {
                let _ = tx.send(builds::fetch_url(&page));
                ctx.request_repaint();
            });
            self.add_build.link_pending = Some((id, url, rx));
            self.add_build.message = None;
        }
        if let Some((id, by)) = shift {
            self.builds.shift(&id, by);
        }
        if let Some(id) = activate {
            self.activate_build(&id);
        }
        if let Some(id) = remove {
            let was_active = self.builds.active().is_some_and(|b| b.id == id);
            self.builds.remove(&id);
            self.add_build.confirm_remove = None;
            if was_active {
                match self.builds.active().map(|b| b.id.clone()) {
                    Some(next) => self.activate_build(&next),
                    None => {
                        if let Some(c) = &mut self.coach {
                            c.reload_steps(&[]);
                        }
                    }
                }
            }
        }
        if closed {
            self.add_build.open = false;
            self.add_build.confirm_remove = None;
        }
    }

    /// Charge l'équipement, l'historique et le build du personnage détecté.
    fn switch_character(&mut self, name: Option<String>) {
        // Le set d'armes affiché ne change pas avec le personnage ; l'aperçu d'étape, si.
        let set = self.gear.set;
        self.step_preview = None;
        if name == self.character {
            return;
        }
        if let Some(n) = &name {
            let dir = character_dir(&self.data_dir, n);
            // Reprise unique des données d'avant le suivi par personnage.
            if !dir.exists() {
                let _ = std::fs::create_dir_all(&dir);
                let has_other = std::fs::read_dir(self.data_dir.join("characters")).map(|d| d.count() > 1).unwrap_or(false);
                if !has_other {
                    for f in ["gear.json", "history.json"] {
                        let _ = std::fs::copy(self.data_dir.join(f), dir.join(f));
                    }
                }
            }
            self.gear = Gear::load(&dir.join("gear.json"));
            self.history_path = dir.join("history.json");
        } else {
            self.gear = Gear::load(&self.data_dir.join("gear.json"));
            self.history_path = self.data_dir.join("history.json");
        }
        self.gear.set = set;
        self.history = load_history(&self.history_path);
        self.diagnosed = None;
        self.builds.set_character(name.as_deref());
        if let Some(id) = self.builds.active().map(|b| b.id.clone()) {
            let files = self.builds.files(&id);
            if let Some(c) = &mut self.coach {
                c.reload_steps(&files);
            }
        }
        self.character = name;
    }

    fn say(&mut self, msg: String) {
        self.status = Some((msg, Instant::now()));
    }

    fn on_copy(&mut self, ev: CopyEvent) {
        let Some(it) = item::parse(&ev.text) else { return };
        match ev.kind {
            CopyKind::Diagnose => {
                self.side_tab = views::SideTab::Diagnostic;
                self.say(format!("Diagnostic : {}", it.title()));
                self.remember(&it);
                if self.stored.auto_estimate {
                    self.request_price(&it);
                }
                // Sinon on revient à l'historique, où l'objet vient s'ajouter en tête.
                self.diagnosed = self.stored.open_on_copy.then_some(it);
            }
            CopyKind::Equip => {
                self.tab = views::Tab::Equipment;
                let msg = match self.gear.equip(&it) {
                    Equipped::Placed { slot, replaced } => format!(
                        "{} suivi en {}{}",
                        it.title(),
                        slot.label().to_lowercase(),
                        replaced.map(|r| format!(" (remplace {r})")).unwrap_or_default()
                    ),
                    Equipped::AlreadyThere(slot) => {
                        format!("{} est déjà suivi ({})", it.title(), slot.label().to_lowercase())
                    }
                    Equipped::CharmsFull => {
                        format!("3 charmes déjà suivis et aucun {} : retire-en un (clic droit)", it.base)
                    }
                    Equipped::UnknownClass(c) => format!("Je ne sais pas où se porte un objet « {c} »"),
                };
                match self.gear.save() {
                    Ok(()) => self.say(msg),
                    Err(e) => self.say(format!("Impossible d'enregistrer l'équipement : {e}")),
                }
            }
        }
    }

    /// Objet à estimer sur le trade (bouton « Estimer ») : jamais estimé, ou l'estimation a échoué.
    fn needs_estimate(&self, it: &Item) -> bool {
        self.stored.trade_prices
            && trade::priceable(it)
            && matches!(self.trade.get(it), None | Some(trade::Estimate::Failed(_)))
    }

    /// Estimation sur le site de trade, si le réglage la permet et que l'objet s'y prête.
    fn request_price(&self, it: &Item) {
        if self.stored.trade_prices && trade::priceable(it) {
            self.trade.request(it);
        }
    }

    /// Valeur marchande : prix réel du marché pour un rare estimé, sinon règles et poe.ninja.
    fn value_of(&self, it: &Item) -> rules::Value {
        let mut value = {
            let data = self.prices.data.lock().unwrap();
            rules::evaluate(it, data.as_ref(), self.market_min_ex)
        };
        if let Some(est @ trade::Estimate::Done { total, .. }) = self.trade.get(it) {
            if let Some(estimate) = est.value_ex().filter(|_| total >= 3) {
                if estimate >= self.market_min_ex {
                    value.label = rules::Label::Market;
                    value.summary = format!("{total} annonces comparables, estimé ≈ {}", self.fmt_price(estimate));
                } else {
                    value.label = rules::Label::Vendor;
                    value.summary = self.below_threshold(estimate);
                }
            } else if total == 0 {
                value.label = rules::Label::Vendor;
                value.summary = "aucune annonce comparable en achat immédiat : sans valeur au marché".into();
            }
        }
        // Unique bon marché : même explication que pour un rare.
        if value.label == rules::Label::Vendor && it.rarity == item::Rarity::Unique {
            if let Ok((ex, _)) = self.price_ex(it) {
                value.summary = self.below_threshold(ex);
            }
        }
        value
    }

    /// Un objet classé « PNJ » malgré un prix : il vaut trop peu pour la peine de le vendre.
    fn below_threshold(&self, ex: f64) -> String {
        format!(
            "vaut ≈ {} au marché, sous le seuil de {} : pas la peine de le mettre en vente, à vendre au PNJ",
            self.fmt_price(ex),
            self.fmt_price(self.market_min_ex)
        )
    }

    /// Annonces lues, en cartes : prix, âge, note, puis mods comparés à l'objet diagnostiqué
    /// (vert : l'annonce en a plus, rouge : moins, gris : mod que l'objet n'a pas ; en bas, ce
    /// qui manque à l'annonce). Les annonces proches d'abord ; les autres, en retrait, pèsent
    /// peu dans le prix.
    fn listing_cards(&mut self, ui: &mut egui::Ui, it: &Item, listings: &[trade::Listing], score: f64, shares: Option<Vec<f64>>) {
        let mine: Vec<(String, Option<f64>)> = it.mods.iter().map(|m| trade::template(m)).collect();
        let (green, red, gray) = (Color32::from_rgb(110, 200, 110), Color32::from_rgb(220, 95, 85), Color32::from_gray(140));
        let note = |v: f64| format!("{v:.1}").replace('.', ",");
        ui.add_space(4.0);
        // Les proches d'abord, puis les autres ; chaque groupe du moins cher au plus cher, avec leur
        // part dans le prix et leur image (chargée à la demande).
        let ordered: Vec<(&trade::Listing, Option<f64>)> = listings
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kept)
            .chain(listings.iter().enumerate().filter(|(_, l)| !l.kept))
            .map(|(i, l)| (l, shares.as_ref().and_then(|s| s.get(i).copied())))
            .collect();
        let images: Vec<Option<egui::TextureHandle>> = ordered.iter().map(|(l, _)| (!l.icon.is_empty()).then(|| self.assets.texture_url(l.icon.clone())).flatten()).collect();
        // Autant de colonnes que la largeur tient de cartes d'au moins 280 px ; les cartes prennent
        // toute la largeur (une colonne : toute la colonne).
        const CARD_MIN: f32 = 280.0;
        const PAD: f32 = 16.0 + 2.0;
        let gap = 8.0;
        let width = ui.available_width();
        let columns = (((width + gap) / (CARD_MIN + PAD + gap)).floor() as usize).max(1);
        let card_w = ((width - gap * (columns - 1) as f32) / columns as f32 - PAD).max(120.0);
        let card = |ui: &mut egui::Ui, l: &trade::Listing, share: Option<f64>, image: Option<&egui::TextureHandle>| {
            // Annonce éloignée : toute la carte en retrait.
            let fade = if l.kept || score == 0.0 { 1.0 } else { 0.45 };
            let c = |color: Color32| color.gamma_multiply(fade);
            let rarity = match l.rarity.as_str() {
                "Magic" => item::Rarity::Magic,
                "Unique" => item::Rarity::Unique,
                "Normal" => item::Rarity::Normal,
                _ => item::Rarity::Rare,
            };
            let amount = if (l.amount - l.amount.round()).abs() < 0.05 { format!("{}", l.amount.round()) } else { format!("{:.1}", l.amount) };
            let age = match l.age_hours {
                Some(h) if h < 1.0 => "à l'instant".to_string(),
                Some(h) if h < 48.0 => format!("il y a {} h", h.round()),
                Some(h) => format!("il y a {} j", (h / 24.0).round()),
                None => String::new(),
            };
            let defences: Vec<String> = ["Armour", "Evasion", "Energy Shield"].iter().zip(l.defences).filter(|(_, v)| *v > 0.0).map(|(n, v)| format!("{n} {v:.0}")).collect();
            // Une case de grille est en ligne : la carte remet ses lignes l'une sous l'autre.
            egui::Frame::new().fill(ui::CARD_FILL).stroke(egui::Stroke::new(1.0_f32, c(if l.kept { ui::GOLD_DIM } else { Color32::from_gray(60) }))).corner_radius(4).inner_margin(8).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(card_w);
                    // En-tête comme celui de l'objet : image, nom, base, niveau et défenses ; à droite
                    // le prix et l'âge de l'annonce. Au survol, le détail.
                    let head = ui
                        .horizontal_top(|ui| {
                            match image {
                                Some(tex) => {
                                    let size = tex.size_vec2();
                                    let scale = (56.0 / size.y).min(44.0 / size.x).min(1.0);
                                    ui.add_sized([46.0, 58.0], egui::Image::new((tex.id(), size * scale)).tint(Color32::WHITE.gamma_multiply(fade)));
                                }
                                None => {
                                    ui.add_space(46.0);
                                }
                            }
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                if !l.name.is_empty() {
                                    ui.label(RichText::new(&l.name).color(c(ui::rarity_color(rarity))).size(15.0).strong());
                                }
                                ui.label(RichText::new(&l.base).color(c(ui::rarity_color(rarity))).size(14.0));
                                let mut info = vec![format!("niv. {}", l.item_level)];
                                info.extend(defences.iter().cloned());
                                ui.label(RichText::new(info.join(" · ")).color(c(Color32::GRAY)).size(13.0));
                            });
                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(RichText::new(format!("{amount} {}", l.currency)).color(c(ui::GOLD)).size(17.0).strong());
                                if let Some(ex) = l.ex {
                                    ui.label(RichText::new(format!("≈ {}", self.fmt_price(ex))).color(c(Color32::GRAY)).size(13.0));
                                }
                                if !age.is_empty() {
                                    ui.label(RichText::new(&age).color(c(Color32::GRAY)).size(13.0));
                                }
                            });
                        })
                        .response;
                    head.on_hover_ui(|ui| {
                        let title = if l.name.is_empty() { l.base.clone() } else { format!("{} — {}", l.name, l.base) };
                        ui.label(RichText::new(title).color(ui::rarity_color(rarity)).strong());
                        ui.label(format!("Niveau d'objet {}", l.item_level));
                        if !defences.is_empty() {
                            ui.label(defences.join(" · "));
                        }
                        ui.label(format!("Prix demandé : {amount} {}", l.currency));
                        if let Some(ex) = l.ex {
                            ui.label(self.price_all(ex));
                        }
                        if !age.is_empty() {
                            ui.label(format!("En vente depuis : {}", age.trim_start_matches("il y a ")));
                        }
                    });
                    // Note et part dans le prix, sur une ligne, entre deux traits discrets.
                    let rule = |ui: &mut egui::Ui| {
                        ui.add_space(3.0);
                        let r = ui.available_rect_before_wrap();
                        ui.painter().hline(r.x_range(), r.top(), egui::Stroke::new(1.0_f32, c(Color32::from_gray(48))));
                        ui.add_space(4.0);
                    };
                    if score > 0.0 {
                        // La note en blanc, sa part dans le prix en gris.
                        let text = if l.kept { format!("note {}", note(l.score)) } else { format!("éloignée : note {} ({}×)", note(l.score), note(l.score / score)) };
                        rule(ui);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(text).color(c(Color32::from_gray(225))).size(13.0));
                            if let Some(sh) = share {
                                let pct = sh * 100.0;
                                let part = if pct < 1.0 { "· pèse moins de 1 % dans le prix".to_string() } else { format!("· pèse {} % dans le prix", pct.round()) };
                                ui.label(RichText::new(part).color(c(Color32::GRAY)).size(13.0));
                            }
                        });
                        rule(ui);
                    } else {
                        ui.add_space(4.0);
                    }
                    let mut seen = Vec::new();
                    for m in &l.mods {
                        let (t, v) = trade::template(m);
                        let mut text = m.clone();
                        let color = match mine.iter().find(|(mt, _)| *mt == t) {
                            Some((_, mv)) => {
                                seen.push(t.clone());
                                match (v, mv) {
                                    (Some(a), Some(b)) if (a - b).abs() >= 0.05 => {
                                        let d = a - b;
                                        let d_text = if (d - d.round()).abs() < 0.05 { format!("{}", d.round()) } else { format!("{d:.1}") };
                                        text = format!("{m}  ({}{d_text})", if d > 0.0 { "+" } else { "" });
                                        if d > 0.0 { green } else { red }
                                    }
                                    _ => Color32::from_gray(210),
                                }
                            }
                            None => gray,
                        };
                        ui.label(RichText::new(text).color(c(color)).size(14.0));
                    }
                    // Mods de l'objet absents de l'annonce.
                    for (line, (t, _)) in it.mods.iter().zip(&mine) {
                        if !seen.contains(t) {
                            ui.label(RichText::new(format!("sans : {line}")).color(c(red.gamma_multiply(0.8))).size(14.0).italics());
                        }
                    }
                });
            });
        };
        egui::Grid::new("listings_grid").num_columns(columns).spacing([gap, gap]).show(ui, |ui| {
            for (i, ((l, share), image)) in ordered.iter().zip(&images).enumerate() {
                card(ui, l, *share, image.as_ref());
                if i % columns == columns - 1 {
                    ui.end_row();
                }
            }
        });
    }

    fn feed_panel(&mut self, ui: &mut egui::Ui) {
        // Pense-bête de la route à gauche, fil des événements à droite (résumé du personnage en
        // sous-titre).
        // Deux cartes, détachées du fond : un peu plus claires, bordées, avec de l'air autour.
        // Près des bords de la fenêtre : on reprend une partie de la marge du panneau (8 px).
        let full = ui.available_rect_before_wrap().shrink2(Vec2::new(-4.0, 6.0));
        // Écart entre les cartes égal à l'air au-dessus et au-dessous (marge du panneau + 6).
        let gap = 8.5;
        let left_w = (full.width() * 0.5).min(600.0);
        let left = egui::Rect::from_min_size(full.min, Vec2::new(left_w, full.height()));
        let right = egui::Rect::from_min_max(egui::pos2(left.right() + gap, full.top()), full.max);
        for card in [left, right] {
            ui.painter().rect(card, 6.0, ui::CARD_FILL, egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.55)), egui::StrokeKind::Inside);
        }
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left.shrink2(Vec2::new(12.0, 6.0))), |ui| self.route_panel(ui));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right.shrink2(Vec2::new(12.0, 6.0))), |ui| {
            let summary = self.coach.as_ref().map(|c| c.summary()).unwrap_or_else(|| "chargement du guide…".into());
            ui::section_header_sub(ui, "ACTIVITÉ", &summary);
            self.feed_list(ui);
        });
    }

    /// Contenus facultatifs à ne pas rater (route de leveling), cochés tout seuls via le log.
    fn route_panel(&mut self, ui: &mut egui::Ui) {
        let quest_choices = self.quest_choices();
        let Some(c) = &mut self.coach else { return };
        let (act, items) = c.reminders();
        let label = match act {
            0 => "ROUTE DE LEVELING".to_string(),
            5 => "ROUTE — INTERLUDES".to_string(),
            a if a > 5 => "ROUTE — CAMPAGNE TERMINÉE".to_string(),
            a => format!("ROUTE — ACTE {a}"),
        };
        ui::section_header(ui, &label, None, |_| {});
        let mut toggle: Option<String> = None;
        let out = egui::ScrollArea::vertical().id_salt("route").auto_shrink([false, false]).show(ui, |ui| {
            if items.is_empty() {
                let empty = match act {
                    0 => "Elle s'affichera dès que le journal du jeu aura nommé ton personnage.",
                    a if a > 5 => "Rien de la campagne n'a été oublié.",
                    _ => "Rien à signaler pour cet acte.",
                };
                ui.label(RichText::new(empty).color(Color32::GRAY));
            }
            for (r, done) in items {
                let behind = r.act < act;
                // Couleur selon l'importance seulement : pas de vert ni de rouge, qui diraient
                // « réussi » ou « erreur ».
                let color = if done {
                    Color32::from_gray(110)
                } else {
                    match r.importance.as_str() {
                        "must" => Color32::from_rgb(235, 215, 160),
                        "recommended" => Color32::from_rgb(200, 195, 180),
                        _ => Color32::from_gray(150),
                    }
                };
                // Récompense au choix : celui que le build recommande remplace la liste des
                // options (elle reste au survol), signalé par une étiquette « build ».
                let picks = route::build_choices(r, &quest_choices);
                let pick = picks.iter().map(|p| p.reward.as_str()).collect::<Vec<_>>().join(" · ");
                let mut reward = RichText::new(if pick.is_empty() { &r.reward } else { &pick }).color(color).strong();
                if done {
                    reward = reward.strikethrough();
                }
                // Une ligne par récompense ; la description est au survol.
                let mut checked = done;
                let row = ui.horizontal(|ui| {
                    // Fait : coche verte.
                    if done {
                        let green = Color32::from_rgb(110, 210, 110);
                        let w = &mut ui.visuals_mut().widgets;
                        for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                            s.fg_stroke.color = green;
                        }
                    }
                    let check = ui.checkbox(&mut checked, "");
                    if !pick.is_empty() {
                        ui.label(RichText::new("build ·").color(ui::GOLD_DIM).small());
                    }
                    if behind && !done {
                        // L'« acte 5 » de la route, ce sont les interludes.
                        let when = if r.act == 5 { "interludes".to_string() } else { format!("acte {}", r.act) };
                        ui.label(RichText::new(when).color(Color32::from_rgb(200, 150, 100)).small().italics());
                    }
                    let label = ui.add(egui::Label::new(reward).truncate());
                    (check, label)
                });
                // Fiche au survol, en blocs : la récompense, comment l'obtenir, où, le choix du build.
                let tip = |ui: &mut egui::Ui| {
                    ui.set_max_width(420.0);
                    ui.label(RichText::new(&r.reward).color(color).strong());
                    ui.label(RichText::new(&r.what).color(Color32::LIGHT_GRAY));
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Zone : {}", r.zone)).color(Color32::GRAY).small());
                    let importance = match r.importance.as_str() {
                        "must" => "Indispensable",
                        "recommended" => "Conseillé",
                        _ => "Sur le chemin de l'histoire",
                    };
                    ui.label(RichText::new(importance).color(Color32::GRAY).small());
                    if behind && !done {
                        let when = if r.act == 5 { "Oubliée aux interludes".to_string() } else { format!("Oubliée à l'acte {}", r.act) };
                        ui.label(RichText::new(when).color(Color32::from_rgb(200, 150, 100)).small().italics());
                    }
                    if !picks.is_empty() {
                        ui.separator();
                        ui.label(RichText::new("CHOIX DU BUILD").color(ui::GOLD_DIM).small().strong());
                        for p in &picks {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&p.reward).color(ui::GOLD));
                                ui.label(RichText::new(format!("({})", p.quest)).color(Color32::GRAY).small());
                            });
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new("Cochée toute seule quand le jeu l'annonce ; clique la case pour corriger.").color(Color32::DARK_GRAY).small().italics());
                };
                let (check, label) = row.inner;
                label.on_hover_ui(tip);
                if check.on_hover_ui(tip).changed() {
                    toggle = Some(r.id.clone());
                }
            }
        });
        ui::fade_scroll_on(ui, &out, false, ui::CARD_FILL);
        if let Some(id) = toggle {
            c.toggle_done(&id);
        }
    }

    fn feed_list(&mut self, ui: &mut egui::Ui) {
        let out = egui::ScrollArea::vertical().id_salt("feed").auto_shrink([false, false]).show(ui, |ui| {
            if self.feed.is_empty() {
                ui.label(RichText::new("Les niveaux, étapes du guide, morts et messages privés s'afficheront ici.").color(Color32::GRAY));
            }
            for f in &self.feed {
                use coach::FeedKind::*;
                let color = match f.kind {
                    Step | Ascendancy => Color32::from_rgb(200, 140, 230),
                    Level => Color32::from_rgb(230, 220, 190),
                    Death => Color32::from_rgb(230, 100, 90),
                    Whisper => Color32::from_rgb(110, 160, 240),
                    Character => Color32::from_rgb(230, 190, 90),
                    Reward => Color32::from_rgb(120, 200, 120),
                    Session => Color32::GRAY,
                };
                ui.label(RichText::new(&f.head).color(color).strong());
                for d in &f.details {
                    ui.label(RichText::new(format!("    {d}")).color(Color32::LIGHT_GRAY));
                }
            }
        });
        ui::fade_scroll_on(ui, &out, false, ui::CARD_FILL);
    }

    /// Zone centrale : fond du jeu, barre personnage / build, onglet actif, ligne d'onglets.
    fn central(&mut self, ui: &mut egui::Ui, glow: &HashMap<Slot, Color32>) {
        use ui_art::Part;
        let avail = ui.available_size();
        let scale = ui::canvas_scale(avail.x, avail.y - TAB_H, self.bar_h);
        let pad = ui::top_pad(scale, self.bar_h);
        // Tout est placé dans des rectangles fixes : zone d'affichage, puis onglets juste dessous.
        let (full, _) = ui.allocate_exact_size(
            Vec2::new(avail.x, ui::CANVAS_H * scale + pad + TAB_H),
            egui::Sense::hover(),
        );
        let whole = egui::Rect::from_min_size(full.min, Vec2::new(ui::CANVAS_W * scale, ui::CANVAS_H * scale + pad));
        let tabs = egui::Rect::from_min_size(egui::pos2(full.min.x, whole.max.y), Vec2::new(full.width(), TAB_H));
        // Le fond commence sous l'éventuel décalage laissé pour la barre.
        let area = egui::Rect::from_min_size(whole.min + Vec2::new(0.0, pad), Vec2::new(ui::CANVAS_W, ui::CANVAS_H) * scale);
        let equipment = self.tab == views::Tab::Equipment;
        if let Some(bg) = self.ui_art.get(Part::Background) {
            let painter = ui.painter();
            painter.image(bg.id(), area, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            // Hors équipement : on garde le bandeau du haut et la bordure, le panneau de pierre (les
            // cases d'équipement dessinées) laisse place à un aplat sombre.
            if !equipment {
                painter.rect_filled(ui::place(area, scale, ui::STONE_PANEL), 4.0, ui::STONE_PANEL_FILL);
            }
            if equipment {
                for (part, frame) in [(Part::LifeFrame, ui::LIFE_FRAME), (Part::ManaFrame, ui::MANA_FRAME), (Part::CharmFrame, ui::CHARM_FRAME)] {
                    if let Some(tex) = self.ui_art.get(part) {
                        ui::draw_image(painter, ui::place(area, scale, frame), tex);
                    }
                }
            }
        }
        self.header_bar(ui, whole, area, scale);
        if equipment {
            self.equipment(ui, area, scale, glow);
        } else if self.tab == views::Tab::Stats {
            self.stats_view(ui, ui::place(area, scale, ui::VIEW_AREA));
        } else {
            self.build_view(ui, ui::place(area, scale, ui::VIEW_AREA));
        }
        self.tab_row(ui, tabs);
    }

    fn tab_row(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let tabs: Vec<_> = views::Tab::ALL.iter().map(|t| (*t, t.label())).collect();
        if let Some(tab) = ui::tab_strip(ui, rect, &tabs, self.tab) {
            self.tab = tab;
        }
    }

    /// Onglet Caractéristiques : l'équipement porté face à ce que vise le build, pour le set
    /// d'armes et l'étape choisis, résistances avec la pénalité de la progression.
    fn stats_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let catalog = self.augments.get();
        let no_steps: Vec<guide::Step> = Vec::new();
        let (steps, current, penalty, level) = match &self.coach {
            Some(c) => {
                let penalty = c
                    .character()
                    .map(|(_, ch)| stats::res_penalty(ch.max_act, ch.level))
                    .unwrap_or((0.0, "acte 1".into()));
                (
                    &c.steps,
                    c.current_step_index(),
                    penalty,
                    c.character().map(|(_, ch)| ch.level),
                )
            }
            None => (&no_steps, None, (0.0, "acte 1".into()), None),
        };
        let (view_step, gear) = (&mut self.view_step, &mut self.gear);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            let mut shown = None;
            ui.horizontal(|ui| {
                ui::gold_widgets(ui);
                if !steps.is_empty() {
                    ui.label(RichText::new("Étape").color(ui::GOLD_DIM));
                    shown = Some(views::step_selector(ui, steps, view_step, current));
                    ui.add_space(12.0);
                }
                let h = ui.min_rect().height().max(ui.spacing().interact_size.y);
                ui.label(RichText::new("Set d'armes").color(ui::GOLD_DIM));
                let size = ui::set_tabs_size(ui, h, [false; 2]);
                let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let mut set = gear.set.max(1);
                if ui::set_tabs(ui, r.center(), h, "stats_set", &mut set, [false; 2], true) {
                    gear.set = set;
                }
                // Niveau du personnage, à droite.
                if let Some(level) = level.filter(|l| *l > 0) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(level.to_string())
                                .color(ui::GOLD)
                                .size(18.0)
                                .strong(),
                        );
                        ui.label(RichText::new("Niveau").color(ui::GOLD_DIM));
                    });
                }
            });
            // Trait sous l'en-tête, comme sous les titres de section.
            ui.add_space(6.0);
            let line = ui.available_rect_before_wrap();
            ui.painter().hline(
                line.x_range(),
                line.top(),
                egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.6)),
            );
            ui.add_space(8.0);
            let set = gear.set.max(1);
            let worn = stats::sheet(gear, set);
            let step = shown.map(|i| &steps[i]);
            let build = step.map(|s| {
                stats::build_sheet(
                    s,
                    set,
                    |b| catalog.as_ref()?.defences.get(b).copied(),
                    |b| catalog.as_ref()?.classes.get(b).map(|(c, _)| c.clone()),
                    |b| {
                        catalog
                            .as_ref()
                            .and_then(|c| c.implicits.get(b).cloned())
                            .unwrap_or_default()
                    },
                )
            });
            let title = step
                .map(|s| format!("Build · {}", s.label))
                .unwrap_or_else(|| "Build (aucun)".into());
            views::stats(ui, &worn, build.as_ref(), &title, &penalty);
        });
    }

    /// Onglets Sorts, Talents et Ascendances, pour l'étape choisie.
    fn build_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(coach) = self.coach.as_ref() else {
            ui.put(rect, egui::Label::new(RichText::new("Chargement du guide…").color(Color32::GRAY)));
            return;
        };
        if coach.steps.is_empty() {
            // Sans build : explication et accès direct à l'ajout.
            let mut add = false;
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(rect.height() * 0.35);
                    ui::gold_widgets(ui);
                    ui.label(RichText::new("Aucun build").color(ui::GOLD).heading());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Ajoute le build que tu suis pour voir ici ses sorts, son arbre de talents et son ascendance, étape par étape.").color(Color32::GRAY));
                    ui.label(RichText::new("Colle le lien de sa page sur Mobalytics ou Kami-labs.").color(Color32::GRAY));
                    ui.add_space(10.0);
                    add = ui.button(RichText::new("Ajouter un build…").color(ui::GOLD)).clicked();
                });
            });
            if add {
                self.add_build.open = true;
                self.add_build.message = None;
            }
            return;
        }
        let current = coach.current_step_index();
        let level = coach.level();
        let key = format!("{}:{}", self.builds.active().map(|b| b.id.as_str()).unwrap_or(""), self.view_step.or(current).unwrap_or(0));
        if let Some(b) = self.builds.active() {
            self.tree.use_overrides_file(self.builds.dir_of(&b.id).join("attributes.json"));
        }
        let (worn, prices, league) = (uniques::worn(&self.gear), self.prices.data.clone(), self.league.clone());
        let gem_infos = self.gem_infos.get();
        let (tab, view_step, assets, tree, icons, weapon_set) =
            (self.tab, &mut self.view_step, &mut self.assets, &mut self.tree, &mut self.passive_icons, &mut self.gear.set);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            let mut shown = 0;
            // Style doré pour les sélecteurs seulement : les barres de défilement du contenu
            // gardent le style de l'application.
            ui.horizontal(|ui| {
                ui::gold_widgets(ui);
                if tab != views::Tab::Ascendancy {
                    ui.label(RichText::new("Étape").color(ui::GOLD_DIM));
                    shown = views::step_selector(ui, &coach.steps, view_step, current);
                }
                // Set d'armes, partagé avec l'écran d'équipement.
                if tab == views::Tab::Passives && coach.steps.iter().any(|s| !s.passive_sets.is_empty()) {
                    ui.add_space(12.0);
                    // Même hauteur que la liste d'étape à sa gauche, centré sur la même ligne.
                    let h = ui.min_rect().height();
                    ui.label(RichText::new("Set d'armes").color(ui::GOLD_DIM));
                    let size = ui::set_tabs_size(ui, h, [false; 2]);
                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let mut set = (*weapon_set).max(1);
                    if ui::set_tabs(ui, r.center(), h, "talent_set", &mut set, [false; 2], true) {
                        *weapon_set = set;
                    }
                }
            });
            ui.add_space(6.0);
            let step = &coach.steps[shown];
            let prev = shown.checked_sub(1).map(|i| &coach.steps[i]);
            match tab {
                views::Tab::Skills => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| views::skills(ui, step, prev, level, assets, gem_infos.as_deref()));
                }
                views::Tab::Ascendancy => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| views::ascendancy(ui, &coach.steps, current, coach.catalog()));
                }
                views::Tab::Passives => {
                    let r = ui.available_rect_before_wrap();
                    tree.show(ui, r, step, prev, coach.catalog(), icons, key, (*weapon_set).max(1));
                }
                views::Tab::Uniques => {
                    let prices = prices.lock().unwrap();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| views::uniques(ui, &coach.steps, shown, &worn, prices.as_ref(), &league, assets));
                }
                views::Tab::Equipment | views::Tab::Stats => {}
            }
        });
    }

    fn header_bar(&mut self, ui: &mut egui::Ui, whole: egui::Rect, area: egui::Rect, scale: f32) {
        // Barre personnage / build dans le bandeau vide en haut de l'écran d'équipement.
        // Le bandeau va du haut (décalage compris) au bas de la zone vide du fond.
        let [sx, sy, sw, sh] = ui::HEADER_STRIP;
        let strip = egui::Rect::from_min_max(
            egui::pos2(area.min.x + sx * scale, whole.min.y),
            egui::pos2(area.min.x + (sx + sw) * scale, area.min.y + (sy + sh) * scale),
        );
        let bar = egui::Rect::from_center_size(strip.center(), Vec2::new(strip.width(), self.bar_h));
        let shown = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| self.build_bar(ui));
        // Hauteur réelle de la barre, réutilisée pour la mise en page suivante.
        let measured = shown.response.rect.height();
        if (measured - self.bar_h).abs() > 0.5 {
            self.bar_h = measured;
            ui.ctx().request_repaint();
        }
    }

    /// Cases d'équipement posées sur le fond.
    fn equipment(&mut self, ui: &mut egui::Ui, area: egui::Rect, scale: f32, glow: &HashMap<Slot, Color32>) {
        use ui_art::Part;
        // Sans le fond du jeu, on dessine nos propres cadres.
        let framed = self.ui_art.get(Part::Background).is_none();
        let blocker = self.ui_art.get(Part::CharmBlocker).cloned();
        let mut to_clear = None;
        let two_handed = self.gear.two_handed();
        let charm_slots = self.gear.charm_slots();
        let set = self.gear.set.max(1);
        // Uniques de l'étape en cours pas encore portés : une étoile sur leur case.
        let (wanted, step_label) = match self.judging_step() {
            Some(step) => {
                let prices = self.prices.data.lock().unwrap();
                let base = |n: &str| prices.as_ref()?.unique_by_name(n).map(|(b, _, _)| b.to_string());
                (uniques::missing_by_slot(step, &self.gear, base), step.label.clone())
            }
            None => (vec![], String::new()),
        };
        // Pour l'infobulle de l'étoile : icône et prix au marché de chaque unique.
        let star_info: HashMap<String, (Option<egui::TextureHandle>, Option<String>)> = wanted
            .iter()
            .map(|(_, n)| {
                let price = {
                    let prices = self.prices.data.lock().unwrap();
                    prices.as_ref().and_then(|p| {
                        p.unique_by_name(n)
                            .map(|(_, div, listings)| (p.to_ex(div), listings))
                    })
                };
                let price = price.map(|(ex, listings)| {
                    let doubt = if listings < prices::MIN_LISTINGS {
                        format!(" ({listings} annonces seulement)")
                    } else {
                        String::new()
                    };
                    format!("≈ {} au marché{doubt}", self.fmt_price(ex))
                });
                (n.clone(), (self.assets.texture_unique(n), price))
            })
            .collect();
        let has_star = |slot: Slot| wanted.iter().any(|(s, _)| *s == slot);
        let mut open_uniques = false;
        let mut star = |ui: &mut egui::Ui, rect: egui::Rect, slot: Slot| {
            let names: Vec<ui::WantedUnique> = wanted.iter().filter(|(s, _)| *s == slot).map(|(_, n)| {
                    let (image, price) = star_info.get(n).cloned().unwrap_or_default();
                    ui::WantedUnique {
                        name: n.clone(),
                        image,
                        price,
                    }
                }).collect();
            if !names.is_empty() && ui::wanted_star(ui, rect, slot, &names, &step_label) {
                open_uniques = true;
            }
        };
        // Set d'armes : l'arme et la main gauche affichées suivent le set choisi.
        let marked = [1u8, 2].map(|n| wanted.iter().any(|(s, _)| [Slot::Weapon.in_set(n), Slot::Offhand.in_set(n)].contains(s)));
        let mut new_set = set;
        let [x, y, h] = ui::SET_SWITCH;
        ui::set_tabs(ui, area.min + Vec2::new(x, y) * scale, h * scale, "set_switch", &mut new_set, marked, false);
        for base in Slot::ALL {
            let slot = base.in_set(set);
            let item = self.gear.item(slot);
            let tex = item.as_ref().and_then(|it| self.assets.texture(it));
            let sockets = item.as_ref().and_then(|it| self.socket_view(it, slot));
            let card = self.build_card(item.as_ref(), slot);
            let ghost = if item.is_none() { self.recommended_ghost(slot) } else { None };
            // Sur le fond du jeu, la case tient dans la bordure dessinée autour d'elle.
            let rect = ui::place(area, scale, ui::layout(slot));
            let rect = if framed { rect } else { rect.shrink(ui::slot_inset(slot) * scale) };
            if item.is_none() {
                if base == Slot::Offhand && two_handed {
                    let weapon_tex = self.gear.item(Slot::Weapon.in_set(set)).and_then(|w| self.assets.texture(&w));
                    ui::blocked_slot(ui, rect, slot, ui::Blocked::ByTwoHander(weapon_tex.as_ref()), framed, has_star(slot));
                    star(ui, rect, slot);
                    continue;
                }
                let charm_index = gear::CHARMS.iter().position(|c| *c == slot);
                if let (Some(i), Some(open)) = (charm_index, charm_slots) {
                    if i >= open {
                        ui::blocked_slot(ui, rect, slot, ui::Blocked::ClosedCharm(blocker.as_ref()), framed, false);
                        continue;
                    }
                }
            }
            if let ui::SlotAction::Clear = ui::slot(ui, rect, slot, item.as_ref(), tex.as_ref(), glow.get(&slot).copied(), framed, has_star(slot), sockets.as_ref(), card.as_ref(), ghost.as_ref()) {
                to_clear = Some(slot);
            }
            star(ui, rect, slot);

        }
        self.gear.set = new_set;
        if open_uniques {
            self.tab = views::Tab::Uniques;
            self.view_step = None;
        }
        if let Some(slot) = to_clear {
            self.gear.clear(slot);
            let _ = self.gear.save();
            self.say(format!("{} n'est plus suivi", slot.label()));
        }
    }

    /// Ajoute un objet en tête de l'historique (sans doublon) et l'enregistre.
    fn remember(&mut self, it: &Item) {
        self.history.retain(|h| h.raw != it.raw);
        self.history.insert(0, it.clone());
        self.history.truncate(HISTORY_MAX);
        self.save_history();
    }

    fn save_history(&self) {
        let raws: Vec<&str> = self.history.iter().map(|h| h.raw.as_str()).collect();
        if let Some(dir) = self.history_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.history_path, serde_json::to_string(&raws).unwrap_or_default());
    }

    /// Prix pour l'historique, en exalts (et s'il est estimé d'après des annonces), ou ce qu'on
    /// affiche à la place : « … » en cours, « ? » sans estimation, « — » sans annonce.
    fn price_ex(&self, it: &Item) -> Result<(f64, bool), &'static str> {
        let data = self.prices.data.lock().unwrap();
        let Some(p) = data.as_ref() else { return Err("…") };
        if it.rarity == item::Rarity::Unique {
            return p.unique(&it.name, &it.base).map(|(v, _)| (p.to_ex(v), false)).ok_or("?");
        }
        if let Some(unit) = p.currency(&it.base) {
            return Ok((p.to_ex(unit * it.stack as f64), false));
        }
        match self.trade.get(it) {
            Some(est @ trade::Estimate::Done { total, .. }) if total > 0 => est.value_ex().map(|m| (m, true)).ok_or("?"),
            Some(trade::Estimate::Pending(_)) => Err("…"),
            _ => Err("—"),
        }
    }

    /// Prix tel que l'historique l'affiche : coloré selon la confiance quand il est estimé
    /// d'après les annonces, blanc cassé pour un prix poe.ninja, gris sans valeur.
    fn price_label(&self, it: &Item) -> (String, Color32) {
        match self.price_ex(it) {
            Ok((ex, approx)) => {
                let text = format!("{}{}", if approx { "≈ " } else { "" }, self.fmt_price(ex));
                let color = match self.trade.get(it).filter(|_| approx) {
                    Some(e) => ui::confidence_color(e.confidence_pct()),
                    // Prix poe.ninja : pas d'estimation, donc couleur neutre.
                    None => Color32::from_rgb(225, 220, 210),
                };
                (text, color)
            }
            Err(_) => match self.trade.get(it) {
                Some(trade::Estimate::Done { total: 0, .. }) => ("sans valeur".to_string(), Color32::GRAY),
                Some(trade::Estimate::Done { .. }) => ("incertain".to_string(), Color32::GRAY),
                _ => ("prix inconnu".to_string(), Color32::from_gray(110)),
            },
        }
    }

    /// Devise la plus parlante pour un montant : divines au-delà d'une divine, chaos au-delà
    /// d'une chaos, sinon exalts. Renvoie (valeur de la devise en exalts, abréviation).
    fn price_unit(&self, ex: f64) -> (f64, &'static str) {
        let data = self.prices.data.lock().unwrap();
        let Some(p) = data.as_ref() else { return (1.0, "ex") };
        let chaos = p.currency("Chaos Orb").map(|c| p.to_ex(c)).unwrap_or(0.0);
        if p.exalted_per_divine > 0.0 && ex >= p.exalted_per_divine {
            (p.exalted_per_divine, "div")
        } else if chaos > 1.0 && ex >= chaos {
            (chaos, "chaos")
        } else {
            (1.0, "ex")
        }
    }

    /// Montant dans une devise : entier en exalts, une décimale au plus sinon (« 1,5 chaos »).
    fn fmt_in(ex: f64, (per, unit): (f64, &str)) -> String {
        let v = ex / per;
        let n = if unit == "ex" && v >= 1.0 {
            format!("{}", v.round())
        } else if (v - v.round()).abs() < 0.05 {
            format!("{}", v.round())
        } else {
            format!("{v:.1}").replace('.', ",")
        };
        format!("{n} {unit}")
    }

    /// Prix dans sa devise la plus parlante : « 2 chaos », « 1,5 div », « 12 ex ».
    fn fmt_price(&self, ex: f64) -> String {
        Self::fmt_in(ex, self.price_unit(ex))
    }

    /// Le prix dans les trois devises courantes : « 68,7 ex · 1 chaos · 0,1 div ».
    fn price_all(&self, ex: f64) -> String {
        let data = self.prices.data.lock().unwrap();
        let Some(p) = data.as_ref() else { return format!("{ex:.1} ex") };
        let fmt = |v: f64| {
            if v >= 100.0 {
                format!("{v:.0}")
            } else if v >= 1.0 {
                format!("{v:.1}").replace('.', ",")
            } else if v >= 0.01 {
                format!("{v:.2}").replace('.', ",")
            } else {
                "< 0,01".into()
            }
        };
        let mut parts = vec![format!("{} ex", fmt(ex))];
        if let Some(chaos) = p.currency("Chaos Orb").map(|c| p.to_ex(c)).filter(|c| *c > 0.0) {
            parts.push(format!("{} chaos", fmt(ex / chaos)));
        }
        if p.exalted_per_divine > 0.0 {
            parts.push(format!("{} div", fmt(ex / p.exalted_per_divine)));
        }
        parts.join(" · ")
    }

    fn history_panel(&mut self, ui: &mut egui::Ui) {
        if self.history.is_empty() {
            ui.label(RichText::new("Ctrl+C sur un objet en jeu pour le diagnostiquer : il s'ajoutera ici.").color(Color32::GRAY));
            return;
        }
        let mut open = None;
        let mut remove = None;
        let mut estimate = None;
        // Uniques du build : leur verdict est une étoile.
        let build_uniques: Vec<String> = self.coach.as_ref().map(|c| uniques::summary(&c.steps).into_iter().map(|u| u.name).collect()).unwrap_or_default();
        for (i, it) in self.history.clone().iter().enumerate() {
            // Pas de trait entre les lignes : le fond alterné les sépare, lignes jointives.
            if i > 0 {
                ui.add_space(-ui.spacing().item_spacing.y);
            }
            let selected = self.diagnosed.as_ref().is_some_and(|d| d.raw == it.raw);
            let comparison = self.compare(it);
            let value = self.value_of(it);
            let mut estimate_rect = None;
            // Fond très légèrement plus clair une ligne sur deux (le panneau est en 12, 11, 10), posé
            // sous le contenu une fois sa taille connue.
            // Le fond déborde de la ligne jusqu'aux bords du panneau (marges du panneau à gauche,
            // de la colonne à droite) : le prix garde de l'air, sans coins arrondis.
            let band_painter = ui
                .painter()
                .with_clip_rect(ui.clip_rect().expand2(Vec2::new(8.0, 0.0)));
            let stripe = band_painter.add(egui::Shape::Noop);
            let row = ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                ui.set_min_width(ui.available_width());
                match self.assets.texture(it) {
                    Some(tex) => {
                        let size = tex.size_vec2();
                        let scale = (32.0 / size.y).min(32.0 / size.x);
                        ui.add_sized([34.0, 34.0], egui::Image::new((tex.id(), size * scale)));
                    }
                    None => {
                        ui.add_space(38.0);
                    }
                }
                // Type de la pièce sur la ligne du nom quand la place le permet (nom, type et prix
                // sans se toucher), sinon dessous.
                let kind = kind_label(it);
                let text_w = |text: &str, style: egui::TextStyle| {
                    ui.painter()
                        .layout_no_wrap(text.to_string(), style.resolve(ui.style()), Color32::WHITE)
                        .size()
                        .x
                };
                let inline = text_w(&it.name, egui::TextStyle::Body)
                    + text_w(&kind, egui::TextStyle::Small)
                    + 40.0
                    + 130.0
                    < ui.available_width();
                // Trois couleurs seulement : la rareté (comme en jeu), le verdict et le prix.
                ui.vertical(|ui| {
                    // Une seule ligne : centrée sur la hauteur de la ligne d'historique.
                    if inline {
                        ui.add_space(
                            ((38.0 - ui.text_style_height(&egui::TextStyle::Body)) / 2.0).max(0.0),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        let unique = it.rarity == item::Rarity::Unique && build_uniques.contains(&it.name);
                        let glyph = if unique {
                            Some(("★", ui::rarity_color(item::Rarity::Unique)))
                        } else {
                            comparison.as_ref().map(|c| {
                                let (text, color) = ui::verdict_style(c.verdict);
                                (text.split(' ').next().unwrap_or(text), color)
                            })
                        };
                        if let Some((g, color)) = glyph {
                            ui.label(RichText::new(g).color(color).strong());
                        }
                        ui.add(egui::Label::new(RichText::new(&it.name).color(ui::rarity_color(it.rarity))).truncate());
                        if inline {
                            ui.add_space(4.0);
                            ui.label(RichText::new(&kind).color(Color32::GRAY).small());
                        }
                    });
                    if !inline {
                        ui.label(RichText::new(&kind).color(Color32::GRAY).small());
                    }
                });
                // Prix estimé, aligné à droite.
                let pending = match self.trade.get(it) {
                    Some(trade::Estimate::Pending(m)) => Some(m),
                    _ => None,
                };
                if self.needs_estimate(it) || pending.is_some() {
                    // Bouton ou loader : centrés sur la hauteur de la ligne.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match pending {
                            None => estimate_rect = Some(ui::action_button(ui, "💰  Estimer").rect),
                            Some(msg) => {
                                // Attente imposée par les limites de l'API : décompte en secondes.
                                if let Some(left) = trade::wait_left() {
                                    ui.label(
                                        RichText::new(format!("{} s", left.as_secs() + 1))
                                            .color(ui::GOLD),
                                    );
                                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                                }
                                ui.add(egui::Spinner::new().size(18.0).color(ui::GOLD))
                                    .on_hover_text(msg);
                            }
                        }
                    });
                    return;
                }
                // Une seule ligne, centrée : le prix, coloré selon la confiance (voir `price_label`).
                let (text, color) = self.price_label(it);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(text).color(color).size(17.0).strong());
                });
            });
            if i % 2 == 1 {
                band_painter.set(
                    stripe,
                    egui::Shape::rect_filled(
                        row.response.rect.expand2(Vec2::new(8.0, 0.0)),
                        0.0,
                        Color32::from_rgb(21, 20, 18),
                    ),
                );
            }
            let resp = ui.interact(row.response.rect, ui.id().with(("history", i)), egui::Sense::click(),
            );
            // Au survol : de quoi décider sans ouvrir le diagnostic.
            let price = match self.price_ex(it) {
                Ok((ex, approx)) => {
                    format!("{}{}", if approx { "≈ " } else { "" }, self.price_all(ex))
                }
                _ if self.needs_estimate(it) => "pas encore estimé : bouton « Estimer »".into(),
                Err("…") => "en cours…".into(),
                _ if matches!(
                    self.trade.get(it),
                    Some(trade::Estimate::Done { total: 0, .. })
                ) =>
                {
                    "sans valeur : aucune annonce comparable en achat immédiat".into()
                }
                Err("?") => "trop peu d'annonces pour estimer".into(),
                Err(_) => "inconnu".into(),
            };
            let confidence = self.trade.get(it).filter(|_| matches!(self.price_ex(it), Ok((_, true)))).map(|e| e.confidence_pct());
            let resp = resp.on_hover_ui(|ui| {
                ui.set_max_width(480.0);
                ui.spacing_mut().item_spacing.y = 5.0;
                let body = |t: &str| RichText::new(t).size(16.0);
                // Le nom et le type sont déjà sur la ligne : on commence par le verdict.
                if let Some(c) = &comparison {
                    let (text, color) = ui::verdict_style(c.verdict);
                    let against = if c.against_name.is_empty() || c.verdict == compare::Verdict::Same { String::new() } else { format!(" que {}", c.against_name) };
                    ui.label(body(&format!("{}{against}", text.to_lowercase())).color(color));
                    ui.add_space(6.0);
                }
                let advice = value.label.advice();
                ui.horizontal(|ui| {
                    ui.label(body("Conseil :").color(Color32::from_gray(225)).strong());
                    ui.label(body(advice).color(ui::value_color(value.label)).strong());
                });
                ui.horizontal(|ui| {
                    ui.label(body("Prix estimé :").color(Color32::from_gray(225)).strong());
                    ui.label(body(&price).color(ui::GOLD));
                });
                if let Some(p) = confidence {
                    ui.horizontal(|ui| {
                        ui.label(body("Confiance :").color(Color32::from_gray(225)).strong());
                        ui.label(body(&format!("{p} %")).color(ui::confidence_color(p)));
                    });
                }
                ui.label(body(&value.summary).color(Color32::LIGHT_GRAY));
                for r in &value.reasons {
                    ui.label(body(&format!("• {r}")).color(Color32::GRAY));
                }
                ui.add_space(6.0);
                ui.label(RichText::new("clic : ouvrir le diagnostic · clic droit : retirer").size(14.0).color(Color32::from_gray(110)).italics());
            });
            if selected || resp.hovered() {
                band_painter.rect_filled(
                    row.response.rect.expand2(Vec2::new(8.0, 0.0)),
                    0.0,
                    Color32::from_white_alpha(if selected { 14 } else { 8 }),
                );
            }
            // La ligne entière est cliquable, par-dessus le bouton : on regarde où était le clic.
            let on_button = estimate_rect
                .zip(resp.interact_pointer_pos())
                .is_some_and(|(r, p)| r.contains(p));
            if resp.clicked() {
                if on_button {
                    estimate = Some(it.clone());
                } else {
                    open = Some(it.clone());
                }
            }
            resp.context_menu(|ui| {
                if ui.button("Retirer de l'historique").clicked() {
                    remove = Some(i);
                    ui.close_menu();
                }
            });
        }
        if let Some(it) = open {
            self.diagnosed = Some(it);
        }
        if let Some(it) = estimate {
            self.request_price(&it);
        }
        if let Some(i) = remove {
            self.history.remove(i);
            self.save_history();
        }
    }

    /// Renvoie le verdict à afficher en haut à droite : il est dessiné par-dessus la zone qui
    /// défile, pour rester visible.
    /// En-tête d'un objet : image, nom, base, défenses de base, implicites, niveau requis ; la
    /// fiche complète au survol. `verdict` : place gardée à droite pour le badge (dessiné à part,
    /// il ne défile pas).
    fn item_preview(&mut self, ui: &mut egui::Ui, it: &Item, verdict: Option<(&'static str, Color32)>) {
        // Survol de l'aperçu = fiche complète en infobulle (rien ne se déplie dans le panneau).
        // L'image prend la hauteur du texte à côté (mesurée à l'affichage précédent) : elle ne
        // grandit pas l'en-tête.
        let base_defences = self.base_defences_text(it);
        let text_h_id = ui.id().with("diag_text_h");
        let text_h: f32 = ui.data(|d| d.get_temp(text_h_id)).unwrap_or(80.0);
        let preview = ui.horizontal(|ui| {
            if let Some(tex) = self.assets.texture(it) {
                let size = tex.size_vec2();
                let scale = (text_h / size.y).min(80.0 / size.x).min(1.5);
                ui.image((tex.id(), size * scale));
            }
            let text = ui.vertical(|ui| {
                ui.label(RichText::new(&it.name).color(ui::rarity_color(it.rarity)).size(17.0).strong());
                if it.base != it.name {
                    ui.label(RichText::new(&it.base).color(ui::rarity_color(it.rarity)));
                }
                // Défenses de base du type, puis implicites.
                if let Some(d) = base_defences {
                    ui.label(RichText::new(d).color(Color32::GRAY).small());
                }
                for m in it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k == item::ModKind::Implicit).map(|(m, _)| m) {
                    ui.label(RichText::new(m).color(Color32::from_rgb(136, 136, 255)).small());
                }
                if it.req_level > 0 {
                    ui.label(RichText::new(format!("Niveau requis {}", it.req_level)).color(Color32::GRAY));
                }
                ui.label(RichText::new("survole pour la fiche complète").color(Color32::DARK_GRAY).italics());
            });
            let h = text.response.rect.height();
            if (h - text_h).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(text_h_id, h));
                ui.ctx().request_repaint();
            }
            // Place gardée pour le verdict, dessiné à part (il ne défile pas).
            if let Some((text, color)) = verdict {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| ui.scope_builder(egui::UiBuilder::new().invisible(), |ui| ui::badge(ui, text, color)));
            }
        })
        .response;
        ui.interact(preview.rect, ui.id().with("preview"), egui::Sense::hover()).on_hover_ui(|ui| ui::item_card(ui, it));
    }

    fn diagnostic(&mut self, ui: &mut egui::Ui, comparison: Option<&compare::Comparison>) -> Option<(&'static str, Color32)> {
        // Sans objet ouvert : l'historique. Avec : le détail, qui le remplace.
        let Some(it) = self.diagnosed.clone() else {
            self.history_panel(ui);
            return None;
        };
        ui.add_space(4.0);
        // Unique recommandé par le build : il prend la place du verdict, la comparaison de ses
        // stats ne dit rien de son effet.
        let judged = self.judging_step().map(|s| s.label.clone());
        let recommended = (it.rarity == item::Rarity::Unique)
            .then(|| self.coach.as_ref())
            .flatten()
            .and_then(|c| {
                let steps = uniques::summary(&c.steps).into_iter().find(|s| s.name == it.name)?.steps;
                let current = judged.as_ref().is_some_and(|l| steps.contains(l));
                Some((steps, current))
            });
        // Verdict en haut à droite de l'aperçu.
        let verdict = match (&recommended, comparison) {
            (Some(_), _) => Some(("★ UNIQUE DU BUILD", ui::rarity_color(item::Rarity::Unique))),
            (None, Some(c)) => Some(ui::verdict_style(c.verdict)),
            (None, None) => None,
        };
        self.item_preview(ui, &it, verdict);
        // Châsses de la pièce : augments déjà posés (leurs stats sont dans les comparaisons) et
        // châsses vides.
        if let Some(sk) = self.socketed(&it) {
            ui.add_space(4.0);
            if !sk.augments.is_empty() || !sk.unknown.is_empty() {
                let mut names = sk.augments.clone();
                names.extend(sk.unknown.iter().map(|l| format!("? ({l})")));
                ui.label(RichText::new(format!("Enchâssé : {}", names.join(", "))).color(Color32::from_rgb(136, 136, 255)));
            }
            if sk.empty() > 0 {
                ui.label(RichText::new(format!("{} châsse(s) vide(s) sur {} : potentiel en plus", sk.empty(), sk.sockets)).color(ui::EMPTY_SOCKET));
            }
        }

        // Zone 2 : comparaison avec la pièce portée.
        match comparison {
            Some(c) => {
                let against = if c.verdict == compare::Verdict::Same { String::new() } else { c.against_name.clone() };
                let title = ui::zone_title(ui, "COMPARÉ À TA PIÈCE", &against);
                if !c.basis.is_empty() {
                    title.on_hover_text("Écarts pesés selon les objectifs du build à cette étape");
                }
                for d in &c.deltas {
                    // Écarts neutres (hors build, mods en plus d'une fiole…) : en retrait.
                    let text = match d.sign {
                        1 => RichText::new(&d.text).color(Color32::from_rgb(110, 210, 110)),
                        -1 => RichText::new(&d.text).color(Color32::from_rgb(230, 100, 90)),
                        _ => RichText::new(&d.text).color(Color32::from_gray(115)),
                    };
                    ui.label(text);
                    if d.headline {
                        ui.add_space(6.0);
                    }
                }
                for n in &c.notes {
                    ui.label(RichText::new(n).color(Color32::GRAY).italics());
                }
                // Augments de la pièce remplacée : perdus avec elle (ils ne se récupèrent pas).
                if c.verdict != compare::Verdict::Same {
                    let worn: Vec<Item> = c.against.iter().filter_map(|s| self.gear.item(*s)).collect();
                    let lost: Vec<String> = worn.iter().filter_map(|w| self.socketed(w)).flat_map(|s| s.augments).collect();
                    if !lost.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("Perdus si tu remplaces ta pièce : {}", lost.join(", "))).color(Color32::from_rgb(230, 150, 90)))
                            .on_hover_text("Un augment posé ne se récupère pas : il disparaît avec la pièce, ou si on en pose un autre par-dessus.");
                    }
                }
            }
            None if recommended.is_none() => {
                ui::zone_title(ui, "COMPARÉ À TA PIÈCE", "");
                ui.label(RichText::new("Ne se porte pas : pas de comparaison.").color(Color32::GRAY));
            }
            None => {}
        }

        // Zone 3 : comparaison avec ce que le build recommande à cet emplacement.
        let slot = comparison.and_then(|c| c.against.first().copied()).or_else(|| self.target_slot(&it));
        let card = slot.and_then(|s| self.build_card(Some(&it), s));
        if recommended.is_some() || card.is_some() {
            ui::zone_title(ui, "SELON LE BUILD", judged.as_deref().unwrap_or_default());
            if let Some((steps, current)) = &recommended {
                let when = if *current { "unique recommandé à cette étape".to_string() } else { format!("unique recommandé à : {}", steps.join(", ")) };
                ui.label(RichText::new(when).color(Color32::from_rgb(200, 170, 110)));
            }
            if let Some(card) = &card {
                // Petite image de la pièce recommandée, à la taille de celles de l'historique.
                ui.horizontal_top(|ui| {
                    if let Some(tex) = &card.image {
                        let size = tex.size_vec2();
                        let scale = (32.0 / size.y).min(32.0 / size.x);
                        ui.add_sized([34.0, 34.0], egui::Image::new((tex.id(), size * scale)));
                    }
                    ui.vertical(|ui| ui::build_card_head(ui, card));
                });
                ui::build_card_lines(ui, card);
            }
        }
        ui.add_space(8.0);
        verdict
    }

    /// « Base : Energy Shield 40 » : défenses du type de base d'une pièce (un objet magique
    /// cache ce type dans son nom), s'il en a.
    fn base_defences_text(&self, it: &Item) -> Option<String> {
        let base = self.assets.index.lock().unwrap().as_ref().and_then(|i| i.base_of(it)).unwrap_or_else(|| it.base.clone());
        let d = self.augments.get()?.defences.get(&base).copied()?;
        let parts: Vec<String> = ["Armour", "Evasion Rating", "Energy Shield"].iter().zip(d).filter(|(_, v)| *v > 0.0).map(|(n, v)| format!("{n} {v:.0}")).collect();
        Some(format!("Base : {}", parts.join(" · ")))
    }

    /// Emplacement où se porterait une pièce (dans le set d'armes affiché).
    fn target_slot(&self, it: &Item) -> Option<Slot> {
        Some(match gear::kind_of(&it.item_class)? {
            gear::Kind::Single(s) => s.in_set(self.gear.set),
            gear::Kind::TwoHanded => Slot::Weapon.in_set(self.gear.set),
            gear::Kind::Ring => Slot::RingLeft,
            gear::Kind::Charm => Slot::Charm1,
        })
    }

    /// Onglet Marché : cours du marché d'échange d'après poe.ninja, par catégorie.
    fn market_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        const CATEGORIES: &[(&str, &str)] = &[
            ("Currency", "Monnaie"),
            ("Fragments", "Fragments"),
            ("Runes", "Runes"),
            ("Essences", "Essences"),
            ("SoulCores", "Cœurs d'âme"),
            ("Idols", "Idoles"),
            ("Expedition", "Expédition"),
            ("Ritual", "Rituel"),
            ("Delirium", "Délire"),
            ("Breach", "Brèche"),
            ("Abyss", "Abîme"),
            ("UncutGems", "Gemmes non taillées"),
            ("LineageSupportGems", "Gemmes de lignée"),
        ];
        // Cours de la ligue regardée : celle du personnage, ou une autre choisie ici.
        let prices = self
            .market_league
            .as_ref()
            .map(|(_, p)| p.clone())
            .unwrap_or_else(|| self.prices.clone());
        let shown_league = self
            .market_league
            .as_ref()
            .map(|(l, _)| l.clone())
            .unwrap_or_else(|| self.league.clone());
        let busy = prices.busy();
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        let age = prices.age();
        // Lignes de la catégorie, filtrées par la recherche, les plus chères d'abord.
        let (rates, mut lines) = {
            let data = prices.data.lock().unwrap();
            let rates = data.as_ref().map(|p| {
                (
                    p.exalted_per_divine,
                    p.currency("Chaos Orb").map(|c| p.to_ex(c)).unwrap_or(0.0),
                )
            });
            let needle = self.market_search.trim().to_lowercase();
            let lines: Vec<prices::MarketLine> = data
                .as_ref()
                .map(|p| {
                    p.market
                        .iter()
                        .filter(|l| {
                            l.category == self.market_category
                                && (needle.is_empty() || l.name.to_lowercase().contains(&needle))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            (rates, lines)
        };
        prices::sort_by_family(&mut lines);
        // Montant en exalts dans la devise la plus parlante, aux taux de cette ligue.
        let fmt = |ex: f64| match rates {
            Some((div, _)) if div > 0.0 && ex >= div => Self::fmt_in(ex, (div, "div")),
            Some((_, chaos)) if chaos > 1.0 && ex >= chaos => Self::fmt_in(ex, (chaos, "chaos")),
            _ => Self::fmt_in(ex, (1.0, "ex")),
        };
        let all = |ex: f64| match rates {
            Some((div, chaos)) if div > 0.0 && chaos > 0.0 => format!(
                "{} · {} · {}",
                Self::fmt_in(ex, (1.0, "ex")),
                Self::fmt_in(ex, (chaos, "chaos")),
                Self::fmt_in(ex, (div, "div"))
            ),
            _ => Self::fmt_in(ex, (1.0, "ex")),
        };
        let mut league_pick: Option<String> = None;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 0,
                    right: 8,
                    top: 0,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    let mut refresh = false;
                    ui::section_header(ui, "MARCHÉ", None, |ui| {
                        if busy {
                            ui.add(egui::Spinner::new().size(16.0).color(ui::GOLD));
                        } else {
                            refresh = ui::action_button(ui, "⟳  Actualiser")
                                .on_hover_text("Recharger les cours depuis poe.ninja")
                                .clicked();
                        }
                        if let Some(a) = age {
                            let text = if a < 90 {
                                "à l'instant".to_string()
                            } else {
                                format!("il y a {} min", a / 60)
                            };
                            ui.label(RichText::new(text).color(Color32::GRAY).small());
                        }
                    });
                    if refresh {
                        prices.refresh();
                    }
                    // Taux de change de référence, et la ligue regardée (toujours là, même sans cours).
                    let rates_text = rates.map(|(ex_per_div, chaos_ex)| {
                        let mut parts = vec![format!("1 div = {} ex", ex_per_div.round())];
                        if chaos_ex > 0.0 {
                            parts.push(format!("1 div = {:.0} chaos", ex_per_div / chaos_ex));
                            parts.push(format!(
                                "1 chaos = {} ex",
                                Self::fmt_in(chaos_ex, (1.0, "")).trim()
                            ));
                        }
                        parts.join("  ·  ")
                    });
                    {
                        ui.horizontal(|ui| {
                            if let Some(t) = &rates_text {
                                ui.label(RichText::new(t).color(Color32::LIGHT_GRAY));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui::gold_widgets(ui);
                                    let mut list = self.leagues.list();
                                    if !list.contains(&shown_league) {
                                        list.insert(0, shown_league.clone());
                                    }
                                    egui::ComboBox::from_id_salt("market_league")
                                        .selected_text(RichText::new(&shown_league).color(ui::GOLD))
                                        .show_ui(ui, |ui| {
                                            for l in list {
                                                let mine = if l == self.league {
                                                    format!("{l} (personnage)")
                                                } else {
                                                    l.clone()
                                                };
                                                if ui
                                                    .selectable_label(l == shown_league, mine)
                                                    .clicked()
                                                {
                                                    league_pick = Some(l);
                                                }
                                            }
                                        })
                                        .response
                                        .on_hover_text("Ligue dont les cours sont affichés");
                                },
                            );
                        });
                        ui.add_space(6.0);
                    }
                    ui.horizontal(|ui| {
                        ui::gold_widgets(ui);
                        let shown = CATEGORIES
                            .iter()
                            .find(|(k, _)| *k == self.market_category)
                            .map(|(_, l)| *l)
                            .unwrap_or("Monnaie");
                        egui::ComboBox::from_id_salt("market_category")
                            .selected_text(RichText::new(shown).color(ui::GOLD))
                            .show_ui(ui, |ui| {
                                for (key, label) in CATEGORIES {
                                    ui.selectable_value(
                                        &mut self.market_category,
                                        key.to_string(),
                                        *label,
                                    );
                                }
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut self.market_search)
                                .hint_text("rechercher")
                                .desired_width(ui.available_width()),
                        );
                    });
                    ui.add_space(6.0);
                    if lines.is_empty() {
                        let text = if age.is_none() {
                            "Chargement des cours…"
                        } else {
                            "Aucun résultat"
                        };
                        ui.label(RichText::new(text).color(Color32::GRAY));
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("market_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (i, l) in lines.iter().enumerate() {
                                // Un peu d'air entre deux familles (orbes de joaillier, essences de glace…).
                                if i > 0 {
                                    if prices::family(&lines[i - 1].name).0
                                        != prices::family(&l.name).0
                                    {
                                        ui.add_space(6.0);
                                    }
                                    ui.separator();
                                }
                                let ex = rates.map(|(r, _)| l.value * r).unwrap_or(0.0);
                                let row = ui.horizontal(|ui| {
                                    ui.set_min_height(30.0);
                                    match self.assets.texture_url(format!(
                                        "{}{}",
                                        prices::IMAGE_HOST,
                                        l.image
                                    )) {
                                        Some(tex) => {
                                            ui.add(egui::Image::new((tex.id(), Vec2::splat(28.0))));
                                        }
                                        None => {
                                            ui.add_space(32.0);
                                        }
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&l.name).color(Color32::from_gray(225)),
                                        )
                                        .truncate(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let (sign, color) = if l.change >= 0.0 {
                                                ("+", Color32::from_rgb(110, 200, 110))
                                            } else {
                                                ("", Color32::from_rgb(220, 95, 85))
                                            };
                                            ui.add_sized(
                                                [56.0, 20.0],
                                                egui::Label::new(
                                                    RichText::new(format!(
                                                        "{sign}{:.0} %",
                                                        l.change
                                                    ))
                                                    .color(color)
                                                    .small(),
                                                ),
                                            );
                                            ui::sparkline(ui, &l.spark, color);
                                            ui.label(
                                                RichText::new(fmt(ex))
                                                    .color(Color32::from_rgb(225, 220, 210))
                                                    .size(16.0)
                                                    .strong(),
                                            );
                                        },
                                    );
                                });
                                row.response.on_hover_ui(|ui| {
                                    ui.label(RichText::new(&l.name).strong());
                                    ui.label(all(ex));
                                    ui.label(format!("Variation sur 7 jours : {:+.1} %", l.change));
                                    ui.label(format!("Volume échangé : {} div", l.volume.round()));
                                });
                            }
                        });
                });
        });
        if let Some(l) = league_pick {
            self.market_league = (l != self.league).then(|| {
                let p = prices::Prices::start(self.dirs.cache_dir().to_path_buf(), l.clone());
                (l, p)
            });
        }
    }

    /// Bas du diagnostic, toujours visible en entier : conseil, prix, confiance, puis comment
    /// le prix a été trouvé.
    /// Bas du diagnostic, toujours visible. Avec une estimation sur le trade : titre
    /// « ESTIMATION » et bouton « Détails » (écran de l'estimation) sur sa ligne, puis prix estimé,
    /// confiance, conseil et « basé sur » ; « Réestimer » en icône devant le titre.
    fn value_panel(&mut self, ui: &mut egui::Ui, it: &Item) {
        let est = if trade::priceable(it) { self.trade.get(it) } else { None };
        // Ce qui a un détail et peut être relancé : une estimation faite sur le trade, ou un prix
        // poe.ninja (relancer = recharger les cours).
        let done = matches!(est, Some(trade::Estimate::Done { .. }));
        let ninja = est.is_none() && self.ninja_price(it).is_some();
        // Une carte, comme la route et l'activité : un peu plus claire que le fond, bordée.
        let (mut again, mut open, mut estimate) = (false, false, false);
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(ui::CARD_FILL)
            .stroke(egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.55)))
            .corner_radius(6)
            .inner_margin(egui::Margin { left: 12, right: 12, top: 6, bottom: 10 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // « Réestimer » en simple icône devant le titre ; « Détails » au bout de la ligne. Même
                // trait dessous que les autres titres de carte.
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if let Some(step) = self.trade.refreshing(it) {
                        // Réestimation en cours : l'ancienne estimation reste affichée.
                        let step = match trade::wait_left() {
                            Some(left) => format!("{step} (limite de l'API : encore {} s)", left.as_secs() + 1),
                            None => step,
                        };
                        ui.add(egui::Spinner::new().size(16.0).color(ui::GOLD)).on_hover_text(step);
                        ui.ctx().request_repaint_after(Duration::from_millis(250));
                        ui.add_space(2.0);
                    } else if done && self.stored.trade_prices {
                        again = ui::action_button(ui, "⟳").on_hover_text("Réestimer : relancer la recherche sur le site de trade (les prix bougent)").clicked();
                        ui.add_space(2.0);
                    } else if ninja {
                        if self.prices.busy() {
                            ui.add(egui::Spinner::new().size(16.0).color(ui::GOLD));
                            ui.ctx().request_repaint_after(Duration::from_millis(250));
                        } else {
                            again = ui::action_button(ui, "⟳").on_hover_text("Recharger les cours de poe.ninja").clicked();
                        }
                        ui.add_space(2.0);
                    }
                    ui.label(RichText::new("ESTIMATION").color(ui::GOLD).size(13.0).strong());
                    // Jamais estimé (ou échec) : le bouton pour lancer l'estimation, à la place de
                    // « Détails ».
                    if self.needs_estimate(it) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (label, hint) = match est {
                                Some(trade::Estimate::Failed(_)) => ("⟳  Réessayer", "Relancer l'estimation sur le site de trade"),
                                _ => ("💰  Estimer", "Cherche des annonces comparables sur le site de trade"),
                            };
                            estimate = ui::action_button(ui, label).on_hover_text(hint).clicked();
                        });
                    }
                    if done || ninja {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let hint = if done { "Comment le prix a été trouvé : recherche, critères, annonces" } else { "D'où vient le prix : cours de poe.ninja" };
                            open = ui::action_button(ui, "🔍  Détails").on_hover_text(hint).clicked();
                        });
                    }
                });
                ui.add_space(2.0);
                let line = ui.available_rect_before_wrap();
                ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.6)));
                ui.add_space(8.0);
                self.value_rows(ui, it, &est);
            });
        if again {
            if done {
                self.trade.request_again(it);
            } else {
                self.prices.refresh();
            }
        }
        if open {
            self.estimate_open = Some(it.raw.clone());
        }
        if estimate {
            self.request_price(it);
        }
    }

    /// Écran de l'estimation d'un unique ou d'une monnaie : le cours de poe.ninja, d'où il vient,
    /// et la recherche de l'unique sur le site de trade.
    fn ninja_detail(&mut self, ui: &mut egui::Ui, it: &Item) {
        if self.ninja_price(it).is_none() {
            ui.label(RichText::new("Pas d'estimation pour cet objet.").color(Color32::GRAY));
            return;
        }
        ui.add_space(4.0);
        self.item_preview(ui, it, None);
        // Le prix : les mêmes lignes qu'au diagnostic.
        self.price_title(ui, it, true);
        self.value_rows(ui, it, &None);
        // La source du prix : sa page sur poe.ninja (sinon, pour un unique, ses annonces).
        let page = self.prices.data.lock().unwrap().as_ref().and_then(|p| p.page_url(it.rarity == item::Rarity::Unique, &it.name, &it.base));
        ui.add_space(10.0);
        ui::zone_title(ui, "SOURCE", "");
        match page {
            Some(url) => {
                ui.hyperlink_to("Voir sur poe.ninja", url);
            }
            None if it.rarity == item::Rarity::Unique => {
                ui.hyperlink_to("Voir les annonces sur le site de trade", uniques::trade_url(&self.league, &it.name, Some(&it.base)));
            }
            None => {}
        }
    }

    /// Prix poe.ninja d'un unique ou d'une monnaie, en exalts, et le nombre d'annonces qui le
    /// fondent (uniques seulement).
    fn ninja_price(&self, it: &Item) -> Option<(f64, Option<u32>)> {
        let data = self.prices.data.lock().unwrap();
        let p = data.as_ref()?;
        if it.rarity == item::Rarity::Unique {
            return p.unique(&it.name, &it.base).map(|(v, n)| (p.to_ex(v), Some(n)));
        }
        p.currency(&it.base).map(|unit| (p.to_ex(unit * it.stack.max(1) as f64), None))
    }

    /// Lignes du bas du diagnostic, dans l'ordre : prix estimé, confiance, conseil, basé sur.
    /// Sans estimation sur le trade, le résumé de la valeur suit.
    fn value_rows(&mut self, ui: &mut egui::Ui, it: &Item, est: &Option<trade::Estimate>) {
        let value = self.value_of(it);
        let gold = Color32::from_rgb(235, 200, 60);
        let row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            // Une ligne trop longue passe à la ligne plutôt que d'être coupée ; en passe de mesure,
            // elle reste sur une ligne (sa vraie largeur).
            let mut inner = |ui: &mut egui::Ui| {
                ui.label(RichText::new(format!("{label} :")).color(Color32::from_gray(225)).strong());
                add(ui);
            };
            if ui.is_sizing_pass() {
                ui.horizontal(|ui| inner(ui));
            } else {
                ui.horizontal_wrapped(|ui| inner(ui));
            }
        };
        let advice = |ui: &mut egui::Ui| {
            row(ui, "Conseil", &mut |ui| {
                ui.label(RichText::new(value.label.advice()).color(ui::value_color(value.label)).strong());
            });
        };
        // En attente d'estimation : les lignes restent, avec « — » en gris.
        let unknown = |ui: &mut egui::Ui| {
            for label in ["Confiance", "Conseil", "Basé sur"] {
                row(ui, label, &mut |ui| {
                    ui.label(RichText::new("—").color(Color32::from_gray(110)));
                });
            }
        };
        match est {
            Some(e @ trade::Estimate::Done { total, prices_ex, base_only, listings, score, .. }) => {
                match (e.value_ex(), e.range_ex()) {
                    (Some(estimate), Some((low, high))) if *total > 0 => {
                        // Fourchette dans la même devise que l'estimation.
                        let unit = self.price_unit(estimate);
                        let span = format!("de {} à {}", Self::fmt_in(low, unit), Self::fmt_in(high, unit));
                        let base = if *base_only { "base " } else { "" };
                        let p = e.confidence_pct();
                        row(ui, "Prix estimé", &mut |ui| {
                            // Confiance faible : la fourchette d'abord, le prix n'est qu'un repère.
                            if p < 50 && high - low >= 0.1 {
                                ui.label(RichText::new(format!("{base}{span}")).color(gold).strong());
                                ui.label(RichText::new(format!("(repère {})", self.fmt_price(estimate))).color(Color32::GRAY));
                            } else {
                                ui.label(RichText::new(format!("{base}≈ {}", self.fmt_price(estimate))).color(gold).strong());
                                let range = if high - low >= 0.1 { format!(" · {span}") } else { String::new() };
                                ui.label(RichText::new(format!("({}){range}", self.price_all(estimate))).color(Color32::GRAY));
                            }
                        });
                        row(ui, "Confiance", &mut |ui| {
                            ui.label(RichText::new(format!("{p} % ({})", e.confidence())).color(ui::confidence_color(p)));
                        });
                    }
                    _ if *total == 0 => row(ui, "Prix estimé", &mut |ui| {
                        ui.label(RichText::new("aucune annonce comparable en achat immédiat").color(Color32::LIGHT_GRAY));
                    }),
                    _ => {
                        let min = prices_ex.iter().copied().fold(f64::INFINITY, f64::min);
                        let from = if min.is_finite() { format!(", dès {}", self.fmt_price(min)) } else { String::new() };
                        let text = format!("trop peu d'annonces pour estimer ({total}{from} ; il en faut {})", trade::MIN_PRICED);
                        row(ui, "Prix estimé", &mut |ui| {
                            ui.label(RichText::new(&text).color(Color32::LIGHT_GRAY));
                        });
                    }
                }
                advice(ui);
                // Sur quoi repose l'estimation (annonces proches sur celles lues, éparpillement).
                let kept = listings.iter().filter(|l| l.kept).count();
                let mut basis = if *score == 0.0 || listings.is_empty() {
                    if *total == 1 { "1 annonce en achat immédiat".to_string() } else { format!("{total} annonces en achat immédiat") }
                } else {
                    format!("{kept} {} sur {} annonces lues", if kept == 1 { "proche" } else { "proches" }, listings.len())
                };
                if let Some(spread) = e.spread().filter(|s| *s >= 2.0) {
                    basis.push_str(&format!(" · prix éparpillés ×{}", spread.round()));
                }
                row(ui, "Basé sur", &mut |ui| {
                    ui.label(RichText::new(&basis).color(Color32::GRAY));
                });

            }
            Some(trade::Estimate::Pending(msg)) => {
                row(ui, "Prix estimé", &mut |ui| {
                    ui.add(egui::Spinner::new().color(ui::GOLD));
                    match trade::wait_left() {
                        Some(left) => {
                            ui.label(RichText::new(format!("limite de l'API de trade : encore {} s", left.as_secs() + 1)).color(Color32::LIGHT_GRAY));
                            ui.ctx().request_repaint_after(Duration::from_millis(250));
                        }
                        None => {
                            ui.label(RichText::new(msg).color(Color32::LIGHT_GRAY));
                        }
                    }
                });
                unknown(ui);
            }
            Some(trade::Estimate::Failed(e)) => {
                row(ui, "Prix estimé", &mut |ui| {
                    ui.label(RichText::new(e).color(Color32::from_rgb(230, 120, 100)));
                });
                unknown(ui);
            }
            None if trade::priceable(it) => {
                row(ui, "Prix estimé", &mut |ui| {
                    if self.stored.trade_prices {
                        ui.label(RichText::new("—").color(Color32::from_gray(110)));
                    } else {
                        ui.label(RichText::new("estimation sur le trade coupée dans les réglages").color(Color32::GRAY));
                    }
                });
                // Estimation coupée : le conseil d'après les règles reste la seule indication.
                if self.stored.trade_prices {
                    unknown(ui);
                } else {
                    advice(ui);
                }
            }
            // Uniques et devises : prix de poe.ninja, présenté comme une estimation.
            None => match self.ninja_price(it) {
                Some((ex, listings)) => {
                    row(ui, "Prix estimé", &mut |ui| {
                        ui.label(RichText::new(self.fmt_price(ex)).color(gold).strong());
                        ui.label(RichText::new(format!("({})", self.price_all(ex))).color(Color32::GRAY));
                    });
                    advice(ui);
                    let basis = match listings {
                        Some(1) => "1 annonce, cours de poe.ninja".to_string(),
                        Some(n) => format!("{n} annonces, cours de poe.ninja"),
                        None => "cours de poe.ninja".to_string(),
                    };
                    row(ui, "Basé sur", &mut |ui| {
                        ui.label(RichText::new(&basis).color(Color32::GRAY));
                    });
                    // Le résumé redit le prix : il est dans le détail.
                    return;
                }
                None => {
                    row(ui, "Prix", &mut |ui| {
                        ui.label(RichText::new("inconnu").color(Color32::GRAY));
                    });
                    advice(ui);
                }
            },
        }
        // Ni conseil ni résumé tant que l'objet attend son estimation : sans prix, « à vendre au
        // PNJ » ou « ne vaut rien » seraient faux. Sinon (estimation coupée, pas de prix connu), le
        // résumé des règles ici.
        let waiting = trade::priceable(it) && self.stored.trade_prices && !matches!(est, Some(trade::Estimate::Done { .. }));
        if !waiting && !matches!(est, Some(trade::Estimate::Done { .. })) {
            ui.add_space(4.0);
            ui.label(RichText::new(&value.summary).color(Color32::LIGHT_GRAY));
            for r in &value.reasons {
                ui.label(RichText::new(format!("• {r}")).color(Color32::GRAY));
            }
        }
    }

    /// Écran de l'estimation : comment le prix a été trouvé (calcul, confiance, valeur), la
    /// recherche faite sur le site de trade et ses critères, puis les annonces lues.
    /// Titre « PRIX » de l'écran d'estimation, précédé de « ⟳ » pour relancer l'estimation
    /// (recherche sur le trade, ou cours de poe.ninja) ; le loader pendant qu'elle tourne.
    fn price_title(&mut self, ui: &mut egui::Ui, it: &Item, ninja: bool) {
        ui.add_space(8.0);
        ui.separator();
        let busy = if ninja { self.prices.busy() } else { matches!(self.trade.get(it), Some(trade::Estimate::Pending(_))) || self.trade.refreshing(it).is_some() };
        let mut again = false;
        ui.horizontal(|ui| {
            if busy {
                ui.add(egui::Spinner::new().size(14.0).color(ui::GOLD));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            } else if ninja || self.stored.trade_prices {
                let hint = if ninja { "Recharger les cours de poe.ninja" } else { "Réestimer : relancer la recherche sur le site de trade (les prix bougent)" };
                again = ui::action_button(ui, "⟳").on_hover_text(hint).clicked();
            }
            ui.label(RichText::new("PRIX").color(ui::GOLD_DIM).small().strong());
        });
        ui.add_space(2.0);
        if again {
            if ninja {
                self.prices.refresh();
            } else {
                self.trade.request_again(it);
            }
        }
    }

    /// Titre de la colonne des annonces lues (fixe au-dessus de leur défilement).
    fn listings_title(&self, ui: &mut egui::Ui, it: &Item) {
        let detail = match self.trade.get(it).filter(|_| trade::priceable(it)) {
            Some(trade::Estimate::Done { listings, total, .. }) => format!("{} sur {total} en vente", listings.len()),
            _ => String::new(),
        };
        // Même titre que les cartes (« ACTIVITÉ ») : sous-titre gris, trait dessous.
        ui::section_header_sub(ui, "ANNONCES LUES", &detail);
    }

    /// `listings_only` : la colonne de droite, les annonces lues seules (sans leur titre) ; sinon
    /// tout le reste.
    fn estimation_detail(&mut self, ui: &mut egui::Ui, it: &Item, listings_only: bool) {
        let Some(e) = self.trade.get(it).filter(|_| trade::priceable(it)) else {
            if listings_only {
                ui.label(RichText::new("Aucune : le prix vient des cours de poe.ninja.").color(Color32::GRAY));
            } else {
                self.ninja_detail(ui, it);
            }
            return;
        };
        let trade::Estimate::Done { url, criteria, base_only, widened, listings, score, .. } = &e else { return };
        let (base_only, widened, score) = (*base_only, *widened, *score);
        if listings_only {
            if listings.is_empty() {
                ui.label(RichText::new("Aucune annonce gardée : réestime l'objet pour les voir.").color(Color32::GRAY));
            } else {
                self.listing_cards(ui, it, listings, score, e.shares());
            }
            return;
        }
        let text = |ui: &mut egui::Ui, t: &str| {
            ui.label(RichText::new(t).color(Color32::GRAY));
        };
        ui.add_space(4.0);
        self.item_preview(ui, it, None);
        // Le prix : les mêmes lignes qu'au diagnostic.
        self.price_title(ui, it, false);
        self.value_rows(ui, it, &Some(e.clone()));
        ui.add_space(10.0);

        ui::zone_title(ui, "RECHERCHE", "");
        if base_only {
            text(ui, "aucun mod reconnu : prix de la base, pour le craft");
        }
        if widened {
            text(ui, &format!("trop peu d'annonces de note proche (±{} %) : les plus proches ont été prises", (trade::SCORE_GAP * 100.0) as u32));
        }
        // Mods cherchés ; ceux lâchés par la recherche élargie, en gris. Le nombre de mods exigé
        // n'est pas répété.
        for c in criteria {
            if c.starts_with("au moins ") && c.ends_with("comme cet objet") {
                continue;
            }
            match c.strip_suffix(" (lâché, sa place reste exigée)") {
                Some(dropped) => ui.label(RichText::new(format!("• {dropped}")).color(Color32::from_gray(110))).on_hover_text("Lâché par la recherche élargie : sa place reste exigée"),
                None => ui.label(RichText::new(format!("• {c}")).color(Color32::LIGHT_GRAY)),
            };
        }
        // La source du prix, comme pour un prix poe.ninja.
        ui.add_space(10.0);
        ui::zone_title(ui, "SOURCE", "");
        ui.hyperlink_to("Voir la recherche sur le site de trade", url);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(ev) = self.copies.try_recv() {
            self.on_copy(ev);
        }
        self.assets.poll(ctx);
        self.ui_art.poll(ctx);
        self.passive_icons.poll(ctx);
        if let Ok(mut c) = self.coach_rx.try_recv() {
            if let Some(id) = self.builds.active().map(|b| b.id.clone()) {
                c.reload_steps(&self.builds.files(&id));
            }
            self.coach = Some(c);
        }
        if let Some(c) = &mut self.coach {
            let mut changed = false;
            while let Ok(ev) = self.log_rx.try_recv() {
                for line in c.handle(&ev) {
                    self.feed.push_front(line);
                    changed = true;
                }
            }
            self.feed.truncate(FEED_MAX);
            if changed {
                c.save();
            }
        }
        let detected = self.coach.as_ref().and_then(|c| c.character().map(|(n, _)| n.to_string()));
        if detected.is_some() && detected != self.character {
            self.switch_character(detected);
        }
        if self.character.is_some() {
            if let Some(it) = self.open_diag.take().and_then(|i| self.history.get(i).cloned()) {
                // Développement : `POE2_TRACKER_ESTIMATE=1` ouvre aussi le détail de l'estimation.
                if std::env::var_os("POE2_TRACKER_ESTIMATE").is_some() {
                    self.estimate_open = Some(it.raw.clone());
                }
                self.diagnosed = Some(it);
            }
        }
        // Message d'état affiché 30 s : un seul réveil pour l'effacer, pas de rafraîchissement continu.
        if let Some((_, at)) = &self.status {
            if let Some(left) = Duration::from_secs(30).checked_sub(at.elapsed()) {
                ctx.request_repaint_after(left);
            }
        }

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(Color32::from_rgb(12, 11, 10)).inner_margin(egui::Margin { left: 12, right: 6, top: 10, bottom: 10 }))
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                match &self.status {
                    Some((msg, at)) if at.elapsed().as_secs() < 30 => {
                        ui.label(RichText::new(msg).color(Color32::from_rgb(220, 200, 150)));
                    }
                    _ => {
                        // Raccourcis en jeu, touches dessinées comme sur un clavier.
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui::keys(ui, &["Ctrl", "C"]);
                        ui.label(RichText::new("diagnostiquer").color(Color32::GRAY));
                        ui.add_space(14.0);
                        ui::keys(ui, &["Ctrl", "Alt", "C"]);
                        ui.label(RichText::new("suivre la pièce").color(Color32::GRAY));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.settings_menu(ui);
                    ui.add_space(6.0);
                    self.vendor_regex_button(ui);
                    ui.add_space(8.0);
                    // Barre du bas : raccourcis en jeu, version et mise à jour, menu ; rien d'autre.
                    self.update_status(ui);
                });
            });
        });
        self.sync_league(ctx);
        self.welcome_window(ctx);
        self.changelog_window(ctx);
        self.settings_window(ctx);
        self.add_build_window(ctx);
        // La grille garde ses proportions et prend toute la place possible ; le diagnostic
        // (à droite) et le fil (en bas) absorbent le reste, sans bande vide autour du fond.
        let free = ctx.available_rect();
        let scale = ui::canvas_scale(free.width() - SIDE_MIN, free.height() - FEED_MIN - TAB_H, self.bar_h);
        let side_w = (free.width() - ui::CANVAS_W * scale).max(SIDE_MIN);
        let feed_h = (free.height() - ui::CANVAS_H * scale - ui::top_pad(scale, self.bar_h) - TAB_H).max(FEED_MIN);
        egui::TopBottomPanel::bottom("feed").resizable(false).exact_height(feed_h).show(ctx, |ui| self.feed_panel(ui));
        let comparison = self.diagnosed.as_ref().and_then(|it| self.compare(it));
        let glow: HashMap<Slot, Color32> = comparison
            .iter()
            .flat_map(|c| c.against.iter().map(|s| (*s, ui::verdict_style(c.verdict).1)))
            .collect();
        egui::SidePanel::right("diagnostic").resizable(false).exact_width(side_w).show(ctx, |ui| {
            // Onglets en bas, sur la même ligne que ceux de gauche ; le contenu au-dessus.
            let side = ui.available_rect_before_wrap();
            let full = egui::Rect::from_min_max(side.min, egui::pos2(side.max.x, side.max.y - TAB_H));
            let tabs_rect = egui::Rect::from_min_max(egui::pos2(side.min.x, full.max.y), side.max);
            let tabs: Vec<_> = views::SideTab::ALL.iter().map(|t| (*t, t.label())).collect();
            if let Some(tab) = ui::tab_strip(ui, tabs_rect, &tabs, self.side_tab) {
                self.side_tab = tab;
            }
            if self.side_tab == views::SideTab::Market {
                self.market_panel(ui, full);
                return;
            }
            // Trois niveaux : historique, diagnostic, détail de l'estimation. Au diagnostic, valeur
            // et estimation sont collées en bas, toujours visibles ; le reste défile au-dessus. La
            // colonne est découpée à la main : la hauteur du bas, mesurée à l'affichage précédent,
            // est retirée à la zone qui défile.
            let diagnosed = self.diagnosed.clone();
            let detail = diagnosed.as_ref().is_some_and(|d| self.estimate_open.as_deref() == Some(d.raw.as_str()));
            let value_h_id = ui.id().with("value_h");
            let value_h: f32 = if diagnosed.is_some() && !detail { ui.data(|d| d.get_temp(value_h_id)).unwrap_or(160.0) } else { 0.0 };
            let split = full.max.y - value_h;
            let top = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, split));
            let scroll = ui
                .allocate_new_ui(egui::UiBuilder::new().max_rect(top), |ui| {
                    // Titre du niveau affiché : il reste en place, le contenu défile dessous.
                    // Fil d'Ariane : HISTORIQUE › DIAGNOSTIC › ESTIMATION ; les niveaux au-dessus
                    // sont soulignés et y ramènent. « Retour » remonte d'un niveau.
                    let mut back = false;
                    let mut goto: Option<usize> = None;
                    let depth = if diagnosed.is_none() { 0 } else if detail { 2 } else { 1 };
                    egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 }).show(ui, |ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let crumbs = ["HISTORIQUE", "DIAGNOSTIC", "ESTIMATION"];
                            for (i, name) in crumbs.iter().enumerate().take(depth + 1) {
                                if i > 0 {
                                    ui.label(RichText::new("›").color(ui::GOLD_DIM).size(14.0));
                                }
                                if i == depth {
                                    ui.label(RichText::new(*name).color(ui::GOLD).size(13.0).strong());
                                    if i == 0 {
                                        ui.label(RichText::new(self.history.len().to_string()).color(ui::GOLD_DIM).size(12.0));
                                    }
                                } else {
                                    let r = ui.add(egui::Label::new(RichText::new(*name).color(ui::GOLD_DIM).size(13.0).strong().underline()).sense(egui::Sense::click()));
                                    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        goto = Some(i);
                                    }
                                }
                            }
                            if depth > 0 {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let hint = if depth == 2 { "Revenir au diagnostic" } else { "Revenir à l'historique" };
                                    back = ui::action_button(ui, "⬅  Retour").on_hover_text(hint).clicked();
                                });
                            }
                        });
                        ui.add_space(2.0);
                        let line = ui.available_rect_before_wrap();
                        ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.6)));
                        ui.add_space(8.0);
                    });
                    if back {
                        goto = Some(depth.saturating_sub(1));
                    }
                    match goto {
                        Some(0) => {
                            self.estimate_open = None;
                            self.diagnosed = None;
                        }
                        Some(1) => self.estimate_open = None,
                        _ => {}
                    }
                    let margin = egui::Margin { left: 0, right: 8, top: 0, bottom: 0 };
                    match (&diagnosed, detail) {
                        // Détail de l'estimation : deux colonnes, chacune avec sa barre de défilement.
                        // À gauche le calcul et la recherche, à droite les annonces lues.
                        // Trop étroit pour deux colonnes : une seule, le calcul au-dessus des annonces.
                        (Some(it), true) if ui.available_width() < 640.0 => egui::ScrollArea::vertical().id_salt("estimate_left").auto_shrink([false, false]).show(ui, |ui| {
                            egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                                self.estimation_detail(ui, it, false);
                                ui.add_space(10.0);
                                self.listings_title(ui, it);
                                self.estimation_detail(ui, it, true);
                            });
                            None
                        }),
                        (Some(it), true) => {
                            let area = ui.available_rect_before_wrap();
                            // La colonne de gauche prend juste la largeur de son contenu sans retour à
                            // la ligne (mesurée à chaque image, en passe invisible) ; celle des annonces
                            // prend le reste, au moins 330 px.
                            let natural = {
                                let mut probe = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(egui::Rect::from_min_size(area.min, Vec2::new(4000.0, area.height())))
                                        .sizing_pass()
                                        .invisible(),
                                );
                                // Texte jamais coupé : la largeur mesurée est celle de la plus longue ligne.
                                probe.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                egui::Frame::new().inner_margin(margin).show(&mut probe, |ui| self.estimation_detail(ui, it, false));
                                probe.min_rect().width() + 16.0
                            };
                            let left_w = natural.min(area.width() - 330.0 - 12.0).max(260.0);
                            let left = egui::Rect::from_min_max(area.min, egui::pos2(area.min.x + left_w, area.max.y));
                            let right = egui::Rect::from_min_max(egui::pos2(left.max.x + 12.0, area.min.y), area.max);
                            let out = ui
                                .allocate_new_ui(egui::UiBuilder::new().max_rect(left), |ui| {
                                    egui::ScrollArea::vertical().id_salt("estimate_left").auto_shrink([false, false]).show(ui, |ui| {
                                        egui::Frame::new().inner_margin(margin).show(ui, |ui| self.estimation_detail(ui, it, false));
                                        None
                                    })
                                })
                                .inner;
                            let listings = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right), |ui| {
                                // Le titre reste en place, les annonces défilent dessous.
                                egui::Frame::new().inner_margin(margin).show(ui, |ui| self.listings_title(ui, it));
                                egui::ScrollArea::vertical().id_salt("estimate_right").auto_shrink([false, false]).show(ui, |ui| {
                                    egui::Frame::new().inner_margin(margin).show(ui, |ui| self.estimation_detail(ui, it, true));
                                })
                            });
                            ui::fade_scroll(ui, &listings.inner, true);
                            out
                        }
                        _ => egui::ScrollArea::vertical().id_salt("diagnostic_scroll").auto_shrink([false, false]).show(ui, |ui| {
                            egui::Frame::new().inner_margin(margin).show(ui, |ui| self.diagnostic(ui, comparison.as_ref())).inner
                        }),
                    }
                })
                .inner;
            ui::fade_scroll(ui, &scroll, true);
            // Verdict du diagnostic, et dessous le prix comme dans l'historique (loader le temps de
            // l'estimation) : à leur place dans l'en-tête, mais fixes, par-dessus ce qui défile.
            if let Some(it) = diagnosed.as_ref().filter(|_| !detail) {
                let anchor = egui::Rect::from_min_max(scroll.inner_rect.min + Vec2::new(0.0, 4.0), egui::pos2(scroll.inner_rect.max.x - 8.0, scroll.inner_rect.min.y + 90.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(anchor).layout(egui::Layout::top_down(egui::Align::Max)), |ui| {
                    if let Some((text, color)) = scroll.inner {
                        ui::badge(ui, text, color);
                        ui.add_space(6.0);
                    }
                    match self.trade.get(it) {
                        // Pas encore estimé : le bouton est dans la carte du bas, pas ici.
                        _ if self.needs_estimate(it) => {}
                        Some(trade::Estimate::Pending(msg)) => {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(18.0).color(ui::GOLD)).on_hover_text(msg);
                                if let Some(left) = trade::wait_left() {
                                    ui.label(RichText::new(format!("{} s", left.as_secs() + 1)).color(ui::GOLD));
                                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                                }
                            });
                        }
                        _ => {
                            let (text, color) = self.price_label(it);
                            ui.label(RichText::new(text).color(color).size(17.0).strong());
                        }
                    }
                });
            }
            if let Some(it) = diagnosed.filter(|_| !detail) {
                let bottom = egui::Rect::from_min_max(egui::pos2(full.min.x, split), full.max);
                let shown = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bottom), |ui| {
                    egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 8 }).show(ui, |ui| self.value_panel(ui, &it));
                });
                let h = shown.response.rect.height();
                if (h - value_h).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(value_h_id, h));
                    ui.ctx().request_repaint();
                }
            }
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(12, 11, 10)))
            .show(ctx, |ui| self.central(ui, &glow));
    }
}

/// Polices système en secours pour les symboles absents de la police par défaut (▲ ▼ ● ≈).
const SYMBOL_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "C:\\Windows\\Fonts\\seguisym.ttf",
];

fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = SYMBOL_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("symbols".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push("symbols".into());
        }
    }
    ctx.set_fonts(fonts);
    // Fiches au survol : un peu plus vite, et même si la souris bouge encore un peu.
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.interaction.tooltip_delay = 0.35;
        s.interaction.show_tooltips_only_when_still = false;
        // Barres de défilement partout pareilles : toujours visibles, à part du contenu (pas
        // par-dessus).
        s.spacing.scroll = egui::style::ScrollStyle::solid();
    });
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(12, 11, 10);
    visuals.window_fill = Color32::from_rgb(8, 8, 8);
    // Toujours sombre, même si le système est en thème clair (Windows par défaut).
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.set_theme(egui::Theme::Dark);
}

fn main() -> eframe::Result {
    updater::remember_exe();
    let dirs = ProjectDirs::from("", "", "poe2-tracker").expect("dossier personnel introuvable");
    let stored = Config::load(&dirs);
    let config = stored.resolved();

    let args: Vec<String> = std::env::args().collect();
    if let Some(result) = asset_tools(&args, &config) {
        if let Err(e) = result {
            eprintln!("erreur : {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // État du guide sans interface : poe2-tracker --status
    if args.iter().any(|a| a == "--status") {
        let catalog = guide::Catalog::load(dirs.cache_dir());
        let store = builds::BuildStore::open(dirs.data_dir());
        let Some(active) = store.active().cloned() else {
            println!("aucun build : ajoute-en un depuis l'interface");
            return Ok(());
        };
        let steps = guide::load_steps(&store.files(&active.id), &catalog);
        println!("Build actif : {} ({} étape(s), source : {})", active.name, steps.len(), active.source);
        let mut c = coach::Coach::new(steps, catalog, dirs.data_dir().join("state.json"));
        for ev in logwatch::read_recent(&config.log_path, 8 * 1024 * 1024) {
            for line in c.handle(&ev) {
                println!("  [{:?}] {} {}", line.kind, line.head, line.details.join(" / "));
            }
        }
        println!("{}", c.summary());
        if let Some(step) = c.current_step() {
            for s in &step.skills {
                println!("  - {} (niv. {}+)", s.describe(), s.from_level);
            }
        }
        return Ok(());
    }

    // Estimation de tout un historique : poe2-tracker --price-all <history.json>
    if let Some(i) = args.iter().position(|a| a == "--price-all") {
        let items: Vec<Item> = load_history(std::path::Path::new(args.get(i + 1).expect("fichier attendu"))).into_iter().filter(trade::priceable).collect();
        let league = config.cli_league(dirs.cache_dir());
        let prices = prices::Prices::start(dirs.cache_dir().to_path_buf(), league.clone());
        for _ in 0..60 {
            if prices.data.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        eprintln!("{} objets à estimer", items.len());
        let mut n = 0;
        trade::estimate_all(&items, dirs.cache_dir(), &league, &prices, |it, e| {
            n += 1;
            let line = match &e {
                trade::Estimate::Done { total, relaxed, instant, base_only, .. } => {
                    let value = match (e.value_ex(), e.range_ex()) {
                        (Some(v), Some((lo, hi))) => format!("≈ {v:.1} ex ({lo:.1}–{hi:.1}), confiance {}", e.confidence()),
                        _ => "pas d'estimation".into(),
                    };
                    let how = format!("{total} annonces{}{}{}", if *instant { "" } else { ", en ligne" }, if *relaxed > 0 { format!(", {relaxed} lâché(s)") } else { String::new() }, if *base_only { ", base seule" } else { "" });
                    format!("{value} — {how}")
                }
                trade::Estimate::Failed(err) => format!("échec : {err}"),
                trade::Estimate::Pending(_) => String::new(),
            };
            println!("{n:>2}. {} ({}, {}) : {line}", it.name, it.base, it.item_class);
        });
        return Ok(());
    }
    // Estimation au marché d'un objet copié dans un fichier : poe2-tracker --price <fichier>
    if let Some(i) = args.iter().position(|a| a == "--price") {
        let text = std::fs::read_to_string(args.get(i + 1).expect("fichier attendu")).expect("fichier illisible");
        let it = item::parse(&text).expect("pas un objet PoE2");
        let league = config.cli_league(dirs.cache_dir());
        let prices = prices::Prices::start(dirs.cache_dir().to_path_buf(), league.clone());
        for _ in 0..60 {
            if prices.data.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        println!("{:#?}", trade::estimate_once(&it, dirs.cache_dir(), &league, &prices));
        return Ok(());
    }

    // Regex des marchands pour l'étape suivie : poe2-tracker --vendor-regex
    if args.iter().any(|a| a == "--vendor-regex") {
        let store = builds::BuildStore::open(dirs.data_dir());
        let catalog = guide::Catalog::load(dirs.cache_dir());
        let files = store.active().map(|b| store.files(&b.id)).unwrap_or_default();
        let mut c = coach::Coach::new(guide::load_steps(&files, &catalog), catalog, dirs.data_dir().join("state.json"));
        for ev in logwatch::read_recent(&config.log_path, 8 * 1024 * 1024) {
            c.handle(&ev);
        }
        for step in &c.steps {
            let (re, names) = vendor_regex::build(step);
            println!("{} ({} car.) : {re}\n  {}", step.label, re.chars().count(), names.join(", "));
        }
        return Ok(());
    }
    // Comparaison d'un objet (texte dans un fichier) avec l'équipement suivi : poe2-tracker --compare <fichier>
    if let Some(i) = args.iter().position(|a| a == "--compare") {
        let text = std::fs::read_to_string(args.get(i + 1).expect("fichier attendu")).expect("fichier illisible");
        let it = item::parse(&text).expect("pas un objet PoE2");
        let gear = Gear::load(&dirs.data_dir().join("gear.json"));
        let store = builds::BuildStore::open(dirs.data_dir());
        let catalog = guide::Catalog::load(dirs.cache_dir());
        let files = store.active().map(|b| store.files(&b.id)).unwrap_or_default();
        let mut c = coach::Coach::new(guide::load_steps(&files, &catalog), catalog, dirs.data_dir().join("state.json"));
        for ev in logwatch::read_recent(&config.log_path, 8 * 1024 * 1024) {
            c.handle(&ev);
        }
        let name = store.active().map(|b| b.name.clone()).unwrap_or_default();
        let basis = c.judging_step().map(|s| (name.as_str(), s));
        let bases = augments::Catalog::cached(dirs.cache_dir()).unwrap_or_default();
        println!("{:#?}", compare::compare(&it, &gear, basis, bases.base(&it.base), &|n| bases.base(n)));
        return Ok(());
    }

    std::thread::spawn(desktop::install);
    let mut viewport = egui::ViewportBuilder::default().with_title("poe2-tracker").with_app_id(desktop::APP_ID);
    if let Some(icon) = desktop::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    // Développement : taille de départ imposée (`POE2_TRACKER_SIZE=2400x1250`).
    let size = std::env::var("POE2_TRACKER_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?])))
        .unwrap_or([1280.0, 1000.0]);
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_inner_size(size)
            .with_min_inner_size([1200.0, 1000.0]),
        ..Default::default()
    };
    eframe::run_native("poe2-tracker", options, Box::new(move |cc| Ok(Box::new(App::new(cc, dirs, stored, config)))))
}
