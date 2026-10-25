//! poe2-tracker : suivi de l'équipement et diagnostic des objets pour Path of Exile 2.
//!
//! En jeu : Ctrl+C sur un objet pour le diagnostiquer, Ctrl+Alt+C pour l'ajouter à
//! l'équipement suivi.

// Sous Windows, pas de console noire à côté de la fenêtre.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod art;
mod builds;
mod bundles;
mod capture;
mod coach;
mod compare;
mod gear;
mod guide;
mod item;
mod leagues;
mod logwatch;
mod passive_icons;
mod paths;
mod prices;
mod route;
mod rules;
mod trade;
mod ui;
mod ui_art;
mod updater;
mod views;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
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
}

impl Default for Config {
    fn default() -> Self {
        Config { league: String::new(), game_dir: PathBuf::new(), market_min_ex: 10.0, log_path: PathBuf::new(), welcomed: false }
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
    market_min_ex: f64,
    copies: Receiver<CopyEvent>,
    /// Dernier objet diagnostiqué (Ctrl+C).
    diagnosed: Option<Item>,
    /// Historique des objets diagnostiqués, le plus récent en premier (conservé sur disque).
    history: Vec<Item>,
    history_path: PathBuf,
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
    updater: updater::Updater,
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
}

const FEED_MAX: usize = 60;
/// Largeur minimale du panneau Diagnostic et hauteur minimale du fil d'événements.
const SIDE_MIN: f32 = 300.0;
/// Hauteur de la ligne d'onglets sous la zone d'affichage.
const TAB_H: f32 = 34.0;
const FEED_MIN: f32 = 230.0;
const HISTORY_MAX: usize = 50;

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
    fn new(cc: &eframe::CreationContext, dirs: ProjectDirs, stored: Config, config: Config) -> App {
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
            market_min_ex: config.market_min_ex,
            copies: rx,
            diagnosed: None,
            history: load_history(&dirs.data_dir().join("history.json")),
            history_path: dirs.data_dir().join("history.json"),
            status: None,
            coach: None,
            coach_rx,
            log_rx,
            feed: VecDeque::new(),
            builds,
            add_build: AddBuild::default(),
            data_dir: dirs.data_dir().to_path_buf(),
            character: None,
            bar_h: 34.0,
            tab: match std::env::var("POE2_TRACKER_TAB").as_deref() {
                Ok("sorts") => views::Tab::Skills,
                Ok("talents") => views::Tab::Passives,
                Ok("ascendances") => views::Tab::Ascendancy,
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
            updater: updater::Updater::start(&cc.egui_ctx),
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

    /// Build actif et étape utilisée pour juger l'équipement.
    fn basis(&self) -> Option<(String, guide::Step)> {
        let name = self.builds.active()?.name.clone();
        let step = self.coach.as_ref()?.judging_step()?.clone();
        Some((name, step))
    }

    fn compare(&self, it: &Item) -> Option<compare::Comparison> {
        let basis = self.basis();
        compare::compare(it, &self.gear, basis.as_ref().map(|(n, s)| (n.as_str(), s)))
    }

    /// Active un build et recharge ses étapes dans le guide.
    fn activate_build(&mut self, id: &str) {
        self.builds.set_active(id);
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
            .inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                ui.set_width(width - 22.0);
                ui::gold_widgets(ui);
                ui.horizontal_centered(|ui| self.build_bar_content(ui));
            });
    }

    fn build_bar_content(&mut self, ui: &mut egui::Ui) {
        if let Some(c) = &self.coach {
            let chars = c.characters();
            if !chars.is_empty() {
                let current = c.character().map(|(n, _)| n.to_string());
                let label = match (&current, c.confirmed) {
                    (Some(n), true) => RichText::new(n.clone()).color(ui::GOLD),
                    (Some(n), false) => RichText::new(format!("{n} ?")).color(ui::GOLD).italics(),
                    (None, _) => RichText::new("personnage ?").color(Color32::GRAY),
                };
                let mut picked: Option<String> = None;
                egui::ComboBox::from_id_salt("character_select").width(190.0).selected_text(label).show_ui(ui, |ui| {
                    for (name, class, level) in &chars {
                        if ui.selectable_label(current.as_ref() == Some(name), format!("{name} — {class} niv. {level}")).clicked() {
                            picked = Some(name.clone());
                        }
                    }
                });
                if !c.confirmed && current.is_some() {
                    ui.label(RichText::new("à confirmer").color(Color32::GRAY).small());
                }
                if let Some(name) = picked {
                    if let Some(c) = &mut self.coach {
                        c.set_character(&name);
                    }
                    self.switch_character(Some(name));
                }
                ui.label(RichText::new("·").color(ui::GOLD_DIM));
            }
        }
        let current = self.builds.active().map(|b| b.name.clone());
        let selected = match &current {
            Some(n) => RichText::new(n).color(ui::GOLD),
            None => RichText::new("aucun build").color(Color32::GRAY),
        };
        let mut chosen: Option<String> = None;
        // La liste du build prend la place restante ; l'étape reste à droite.
        let right_part = if self.coach.as_ref().is_some_and(|c| c.character().is_some()) { 210.0 } else { 60.0 };
        let build_w = (ui.available_width() - right_part).max(140.0);
        egui::ComboBox::from_id_salt("build_select").width(build_w).selected_text(selected).show_ui(ui, |ui| {
            for b in self.builds.list() {
                let active = self.builds.active().is_some_and(|a| a.id == b.id);
                if ui.selectable_label(active, &b.name).on_hover_text(format!("source : {}", b.source)).clicked() {
                    chosen = Some(b.id.clone());
                }
            }
        });
        if let Some(id) = chosen {
            self.activate_build(&id);
        }
        let step = self.coach.as_ref().and_then(|c| c.judging_step()).map(|s| s.label.clone());
        let has_character = self.coach.as_ref().is_some_and(|c| c.character().is_some());
        let mut league_pick: Option<String> = None;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(step) = step {
                ui.label(RichText::new(step).color(ui::GOLD_DIM).small());
            }
            if has_character {
                let mut list = self.leagues.list();
                if !list.contains(&self.league) {
                    list.insert(0, self.league.clone());
                }
                egui::ComboBox::from_id_salt("league_select")
                    .width(130.0)
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
            }
        });
        if let (Some(l), Some(c)) = (league_pick, &mut self.coach) {
            c.set_league(l);
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

    /// Bouton « ⚙ Paramètres » de la barre d'état, menu ouvert vers le haut.
    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        ui::gold_widgets(ui);
        let button = ui.button(RichText::new("⚙  Paramètres").color(ui::GOLD));
        let popup = ui.make_persistent_id("settings_popup");
        if button.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup::popup_above_or_below_widget(ui, popup, &button, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(190.0);
            if ui.button("Gestionnaire de builds").clicked() {
                self.add_build.open = true;
                self.add_build.message = None;
            }
            if ui.button("Dossier du jeu…").clicked() {
                self.welcome.open = true;
                self.welcome.path = self.config.game_dir.display().to_string();
                self.welcome.message = None;
            }
            if ui.add_enabled(!self.history.is_empty(), egui::Button::new("Vider l'historique")).clicked() {
                self.history.clear();
                self.save_history();
                self.say("Historique vidé".into());
            }
        });
    }

    /// Fenêtre d'ajout : lien Kami-labs ou direct, ou fichiers glissés dans la fenêtre.
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
        if !self.add_build.open {
            return;
        }
        let mut activate = None;
        let mut remove = None;
        let (_, closed) = ui::modal(ctx, "build_manager", "Gestionnaire de builds", true, 460.0, |ui| {
                // Liste des builds, une action par ligne.
                if self.builds.list().is_empty() {
                    ui.label(RichText::new("Aucun build pour l'instant.").color(Color32::GRAY));
                }
                let active_id = self.builds.active().map(|b| b.id.clone());
                for (i, b) in self.builds.list().iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&b.name).color(ui::GOLD).strong());
                                if active_id.as_ref() == Some(&b.id) {
                                    ui.label(RichText::new("● actif").color(Color32::from_rgb(110, 210, 110)).small());
                                }
                            });
                            ui.label(RichText::new(format!("source : {}", b.source)).color(Color32::GRAY).small());
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
                                if active_id.as_ref() != Some(&b.id) && ui.button("Activer").clicked() {
                                    activate = Some(b.id.clone());
                                }
                            }
                        });
                    });
                }

                ui.add_space(12.0);
                ui::section_header(ui, "AJOUTER UN BUILD", None, |_| {});
                ui.label("Lien d'une page Kami-labs, ou lien direct vers un fichier .build / .zip :");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.add_build.url).hint_text("https://kami-labs.fr/…").desired_width(340.0));
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
                ui.add_space(8.0);
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(90, 80, 60)))
                    .corner_radius(4)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_min_width(436.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("ou glisse ici des fichiers .build ou .zip").strong());
                            ui.label(
                                RichText::new("Mobalytics, Maxroll… bloquent les logiciels : clique « Download Build File » sur la page, puis glisse le fichier téléchargé.")
                                    .color(Color32::GRAY)
                                    .small(),
                            );
                        });
                    });
                if let Some((ok, msg)) = &self.add_build.message {
                    let color = if *ok { Color32::from_rgb(110, 210, 110) } else { Color32::from_rgb(230, 120, 100) };
                    ui.add_space(6.0);
                    ui.label(RichText::new(msg).color(color));
                }
            });
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

    /// Fichiers .build / .zip glissés n'importe où dans la fenêtre.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let mut files = Vec::new();
        let mut errors = Vec::new();
        for f in dropped {
            let name = f.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or(f.name.clone());
            let data = match (&f.path, &f.bytes) {
                (Some(p), _) => std::fs::read(p).map_err(|e| e.to_string()),
                (None, Some(b)) => Ok(b.to_vec()),
                _ => Err("fichier illisible".into()),
            };
            match data.and_then(|d| builds::read_build_files(&name, d)) {
                Ok(mut fs) => files.append(&mut fs),
                Err(e) => errors.push(format!("{name} : {e}")),
            }
        }
        if !errors.is_empty() {
            self.add_build.open = true;
            self.add_build.message = Some((false, errors.join(" ; ")));
        }
        if !files.is_empty() {
            self.import_files(files, "fichier glissé");
        }
    }

    /// Charge l'équipement, l'historique et le build du personnage détecté.
    fn switch_character(&mut self, name: Option<String>) {
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
                self.say(format!("Diagnostic : {}", it.title()));
                if trade::priceable(&it) {
                    self.trade.request(&it);
                }
                self.remember(&it);
                self.diagnosed = Some(it);
            }
            CopyKind::Equip => {
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

    /// Valeur marchande : prix réel du marché pour un rare estimé, sinon règles et poe.ninja.
    fn value_of(&self, it: &Item) -> rules::Value {
        let mut value = {
            let data = self.prices.data.lock().unwrap();
            rules::evaluate(it, data.as_ref(), self.market_min_ex)
        };
        if let Some(est @ trade::Estimate::Done { total, .. }) = self.trade.get(it) {
            if let Some(estimate) = est.value_ex().filter(|_| total >= 3) {
                let label = if estimate >= self.market_min_ex { rules::Label::Market } else { rules::Label::Vendor };
                value.label = label;
                value.summary = format!("{total} annonces comparables, estimé ≈ {estimate:.1} ex");
            } else if total == 0 {
                value.label = rules::Label::Vendor;
                value.summary = "aucune annonce comparable en ligne".into();
            }
        }
        value
    }

    /// Annonces comparables trouvées par l'API de trade.
    fn market_section(&self, ui: &mut egui::Ui, it: &Item) {
        ui.label(RichText::new("Marché (annonces comparables)").color(Color32::GRAY));
        match self.trade.get(it) {
            None => {
                ui.label(RichText::new("pas encore estimé").color(Color32::GRAY));
            }
            Some(trade::Estimate::Pending(msg)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(msg).color(Color32::LIGHT_GRAY));
                });
            }
            Some(trade::Estimate::Failed(e)) => {
                ui.label(RichText::new(e).color(Color32::from_rgb(230, 120, 100)));
            }
            Some(est @ trade::Estimate::Done { .. }) => {
                let trade::Estimate::Done { total, prices_ex, relaxed, url, criteria } = &est else { unreachable!() };
                if *total == 0 {
                    ui.label(RichText::new("Aucune annonce comparable en ligne : probablement sans valeur.").color(Color32::LIGHT_GRAY));
                } else {
                    let min = prices_ex.iter().copied().fold(f64::INFINITY, f64::min);
                    let estimate = est.value_ex().unwrap_or(0.0);
                    ui.label(RichText::new(format!("{total} annonces · dès {min:.1} ex · estimé {estimate:.1} ex")).color(Color32::from_rgb(235, 200, 60)).strong())
                        .on_hover_text("Prix médian des 5 annonces comparables les moins chères (en ligne, même catégorie, mods proches, au moins 85 % des DPS ou de la défense de base).");
                    if *relaxed {
                        ui.label(RichText::new("recherche élargie (tous les mods sauf un)").color(Color32::GRAY).italics());
                    }
                }
                ui.hyperlink_to("Voir la recherche sur le site de trade", url);
                egui::CollapsingHeader::new("Critères utilisés").id_salt("trade_criteria").show(ui, |ui| {
                    for c in criteria {
                        ui.label(RichText::new(c).color(Color32::GRAY));
                    }
                });
            }
        }
    }

    fn feed_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let summary = self.coach.as_ref().map(|c| c.summary()).unwrap_or_else(|| "chargement du guide…".into());
            ui.label(RichText::new(summary).color(Color32::from_rgb(200, 190, 160)));
            if let Some(c) = &mut self.coach {
                // En endgame : choix de l'étape ou de la variante du build (BUDGET, 94% Block…).
                let endgame: Vec<String> = guide::endgame_steps(&c.steps).iter().map(|s| s.label.clone()).collect();
                if c.character().is_some_and(|(_, s)| s.max_act > 5) && !endgame.is_empty() {
                    let current = c.character().map(|(_, s)| s.endgame_step).unwrap_or(1).clamp(1, endgame.len() as u32);
                    let mut chosen = current;
                    egui::ComboBox::from_id_salt("endgame_step")
                        .selected_text(&endgame[chosen as usize - 1])
                        .show_ui(ui, |ui| {
                            for (i, label) in endgame.iter().enumerate() {
                                ui.selectable_value(&mut chosen, i as u32 + 1, label);
                            }
                        });
                    if chosen != current {
                        c.set_endgame_step(chosen);
                    }
                }
            }
        });
        ui.separator();
        // Pense-bête de la route à gauche, fil des événements à droite.
        let width = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new((width * 0.5).min(560.0), ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                self.route_panel(ui);
            });
            ui.separator();
            ui.vertical(|ui| self.feed_list(ui));
        });
    }

    /// Contenus facultatifs à ne pas rater (route de leveling), cochés tout seuls via le log.
    fn route_panel(&mut self, ui: &mut egui::Ui) {
        let Some(c) = &mut self.coach else { return };
        let (act, items) = c.reminders();
        let label = match act {
            0 => "Route de leveling".to_string(),
            5 => "Route — interludes".to_string(),
            a if a > 5 => "Route — campagne terminée".to_string(),
            a => format!("Route — acte {a}"),
        };
        ui.label(RichText::new(label).color(ui::GOLD).strong());
        let mut toggle: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("route").auto_shrink([false, false]).show(ui, |ui| {
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
                let color = if done {
                    Color32::from_gray(110)
                } else if behind {
                    Color32::from_rgb(230, 120, 90)
                } else {
                    match r.importance.as_str() {
                        "must" => Color32::from_rgb(235, 215, 160),
                        "recommended" => Color32::from_rgb(200, 195, 180),
                        _ => Color32::from_gray(150),
                    }
                };
                let (mut reward, mut what) = (RichText::new(&r.reward).color(color).strong(), RichText::new(&r.what).color(Color32::from_gray(if done { 90 } else { 150 })));
                if done {
                    (reward, what) = (reward.strikethrough(), what.strikethrough());
                }
                // Une ligne par récompense : la description est coupée, entière au survol.
                let mut checked = done;
                let row = ui.horizontal(|ui| {
                    let c = ui.checkbox(&mut checked, "").changed();
                    ui.add(egui::Label::new(reward).truncate());
                    ui.add(egui::Label::new(what).truncate());
                    c
                });
                let where_ = if behind { format!("Acte {} · {}", r.act, r.zone) } else { r.zone.clone() };
                let hint = match r.importance.as_str() {
                    "must" => "indispensable",
                    "recommended" => "conseillé",
                    _ => "sur le chemin de l'histoire",
                };
                row.response.on_hover_text(format!("{}\n{where_} ({hint})\nCoché tout seul quand le jeu l'annonce ; case à cocher pour corriger.", r.what));
                if row.inner {
                    toggle = Some(r.id.clone());
                }
            }
        });
        if let Some(id) = toggle {
            c.toggle_done(&id);
        }
    }

    fn feed_list(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("feed").auto_shrink([false, false]).show(ui, |ui| {
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
            // Hors équipement, le fond reste en place mais assombri, sous le contenu de l'onglet.
            let tint = if equipment { Color32::WHITE } else { Color32::from_gray(70) };
            painter.image(bg.id(), area, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), tint);
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
        } else {
            self.build_view(ui, ui::place(area, scale, ui::VIEW_AREA));
        }
        self.tab_row(ui, tabs);
    }

    fn tab_row(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui::gold_widgets(ui);
            ui.horizontal_centered(|ui| {
                ui.add_space(6.0);
                for tab in views::Tab::ALL {
                    let on = self.tab == tab;
                    let text = RichText::new(tab.label()).color(if on { ui::GOLD } else { Color32::GRAY }).size(14.0);
                    if ui.selectable_label(on, text).clicked() {
                        self.tab = tab;
                    }
                    ui.add_space(4.0);
                }
            });
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
                    ui.label(RichText::new("Lien Kami-labs, lien direct ou fichier .build du planificateur glissé dans la fenêtre.").color(Color32::GRAY));
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
        let (tab, view_step, assets, tree, icons) =
            (self.tab, &mut self.view_step, &mut self.assets, &mut self.tree, &mut self.passive_icons);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui::gold_widgets(ui);
            let mut shown = 0;
            ui.horizontal(|ui| {
                if tab != views::Tab::Ascendancy {
                    ui.label(RichText::new("Étape").color(ui::GOLD_DIM));
                    shown = views::step_selector(ui, &coach.steps, view_step, current);
                }
            });
            ui.add_space(6.0);
            let step = &coach.steps[shown];
            let prev = shown.checked_sub(1).map(|i| &coach.steps[i]);
            match tab {
                views::Tab::Skills => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| views::skills(ui, step, prev, level, assets));
                }
                views::Tab::Ascendancy => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| views::ascendancy(ui, &coach.steps, current, coach.catalog()));
                }
                views::Tab::Passives => {
                    let r = ui.available_rect_before_wrap();
                    tree.show(ui, r, step, prev, coach.catalog(), icons, key);
                }
                views::Tab::Equipment => {}
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
        for slot in Slot::ALL {
            let item = self.gear.item(slot);
            let tex = item.as_ref().and_then(|it| self.assets.texture(it));
            let rect = ui::place(area, scale, ui::layout(slot));
            if item.is_none() {
                if slot == Slot::Offhand && two_handed {
                    let weapon_tex = self.gear.item(Slot::Weapon).and_then(|w| self.assets.texture(&w));
                    ui::blocked_slot(ui, rect, slot, ui::Blocked::ByTwoHander(weapon_tex.as_ref()), framed);
                    continue;
                }
                let charm_index = gear::CHARMS.iter().position(|c| *c == slot);
                if let (Some(i), Some(open)) = (charm_index, charm_slots) {
                    if i >= open {
                        ui::blocked_slot(ui, rect, slot, ui::Blocked::ClosedCharm(blocker.as_ref()), framed);
                        continue;
                    }
                }
            }
            if let ui::SlotAction::Clear = ui::slot(ui, rect, slot, item.as_ref(), tex.as_ref(), glow.get(&slot).copied(), framed) {
                to_clear = Some(slot);
            }
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

    /// Prix court pour l'historique : poe.ninja (uniques, devises) ou annonces comparables (rares).
    fn price_text(&self, it: &Item) -> (String, Color32) {
        let gold = Color32::from_rgb(235, 200, 60);
        let dim = Color32::GRAY;
        let short = |ex: f64, p: &prices::PriceData| {
            if p.exalted_per_divine > 0.0 && ex >= p.exalted_per_divine {
                format!("{:.1} div", ex / p.exalted_per_divine)
            } else if ex >= 10.0 {
                format!("{ex:.0} ex")
            } else {
                format!("{ex:.1} ex")
            }
        };
        let data = self.prices.data.lock().unwrap();
        let Some(p) = data.as_ref() else { return ("…".into(), dim) };
        if it.rarity == item::Rarity::Unique {
            return match p.unique(&it.name, &it.base) {
                Some((v, _)) => (short(p.to_ex(v), p), gold),
                None => ("?".into(), dim),
            };
        }
        if let Some(unit) = p.currency(&it.base) {
            return (short(p.to_ex(unit * it.stack as f64), p), gold);
        }
        match self.trade.get(it) {
            Some(est @ trade::Estimate::Done { total, .. }) if total > 0 => match est.value_ex() {
                Some(m) => (format!("≈{}", short(m, p)), gold),
                None => ("?".into(), dim),
            },
            Some(trade::Estimate::Done { .. }) => ("—".into(), dim),
            Some(trade::Estimate::Pending(_)) => ("…".into(), dim),
            _ => ("—".into(), dim),
        }
    }

    /// Liste des diagnostics passés : clic = rouvrir, clic droit = retirer.
    fn history_panel(&mut self, ui: &mut egui::Ui) {
        ui::section_header(ui, "HISTORIQUE", Some(self.history.len()), |_| {});
        if self.history.is_empty() {
            ui.label(RichText::new("Ctrl+C sur un objet en jeu pour le diagnostiquer : il s'ajoutera ici.").color(Color32::GRAY));
            return;
        }
        let mut open = None;
        let mut remove = None;
        for (i, it) in self.history.clone().iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            let selected = self.diagnosed.as_ref().is_some_and(|d| d.raw == it.raw);
            let row = ui.horizontal(|ui| {
                ui.set_min_height(34.0);
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
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&it.name).color(ui::rarity_color(it.rarity)));
                        if let Some(c) = self.compare(it) {
                            let (text, color) = ui::verdict_style(c.verdict);
                            ui.label(RichText::new(text).color(color).small().strong());
                        }
                    });
                    let value = self.value_of(it);
                    ui.label(RichText::new(value.label.text()).color(ui::value_color(value.label)).small());
                });
                // Prix estimé, aligné à droite.
                let (price, price_color) = self.price_text(it);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(price).color(price_color).strong());
                });
            });
            let resp = ui.interact(row.response.rect, ui.id().with(("history", i)), egui::Sense::click());
            if selected || resp.hovered() {
                ui.painter().rect_filled(row.response.rect, 3.0, Color32::from_white_alpha(if selected { 14 } else { 8 }));
            }
            if resp.clicked() {
                open = Some(it.clone());
            }
            resp.context_menu(|ui| {
                if ui.button("Retirer de l'historique").clicked() {
                    remove = Some(i);
                    ui.close_menu();
                }
            });
        }
        if let Some(it) = open {
            if trade::priceable(&it) {
                self.trade.request(&it);
            }
            self.diagnosed = Some(it);
        }
        if let Some(i) = remove {
            self.history.remove(i);
            self.save_history();
        }
    }

    fn diagnostic(&mut self, ui: &mut egui::Ui, comparison: Option<&compare::Comparison>) {
        // Sans objet ouvert : l'historique. Avec : le détail, qui le remplace.
        let Some(it) = self.diagnosed.clone() else {
            self.history_panel(ui);
            return;
        };
        let mut back = false;
        ui::section_header(ui, "DIAGNOSTIC", None, |ui| {
            back = ui::text_button(ui, "← retour").on_hover_text("Revenir à l'historique").clicked();
        });
        if back {
            self.diagnosed = None;
            return;
        }
        ui.add_space(4.0);
        // Survol de l'aperçu = fiche complète en infobulle (rien ne se déplie dans le panneau).
        let preview = ui.horizontal(|ui| {
            if let Some(tex) = self.assets.texture(&it) {
                let size = tex.size_vec2();
                let scale = (110.0 / size.y).min(80.0 / size.x).min(1.5);
                ui.image((tex.id(), size * scale));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(&it.name).color(ui::rarity_color(it.rarity)).size(17.0).strong());
                if it.base != it.name {
                    ui.label(RichText::new(&it.base).color(ui::rarity_color(it.rarity)));
                }
                if it.req_level > 0 {
                    ui.label(RichText::new(format!("Niveau requis {}", it.req_level)).color(Color32::GRAY));
                }
                ui.label(RichText::new("survole pour la fiche complète").color(Color32::DARK_GRAY).italics());
            });
        })
        .response;
        ui.interact(preview.rect, ui.id().with("preview"), egui::Sense::hover()).on_hover_ui(|ui| ui::item_card(ui, &it));
        ui.add_space(6.0);

        // Comparaison avec la pièce suivie.
        match comparison {
            Some(c) => {
                let (text, color) = ui::verdict_style(c.verdict);
                ui::badge(ui, text, color);
                if !c.against_name.is_empty() && c.verdict != compare::Verdict::Same {
                    ui.label(RichText::new(format!("comparé à {}", c.against_name)).color(Color32::GRAY));
                }
                if !c.basis.is_empty() {
                    ui.label(RichText::new(format!("selon {}", c.basis)).color(Color32::from_rgb(200, 170, 110)));
                }
                for d in &c.deltas {
                    let col = match d.sign {
                        1 => Color32::from_rgb(110, 210, 110),
                        -1 => Color32::from_rgb(230, 100, 90),
                        _ => Color32::LIGHT_GRAY,
                    };
                    ui.label(RichText::new(&d.text).color(col));
                }
                for n in &c.notes {
                    ui.label(RichText::new(n).color(Color32::GRAY).italics());
                }
                if !c.against.is_empty() {
                    if let Some((_, step)) = self.basis() {
                        let wanted: Vec<String> = c.against.iter().flat_map(|s| step.targets_for(*s)).map(|t| t.line.clone()).collect();
                        if !wanted.is_empty() {
                            ui.add_space(4.0);
                            ui.label(RichText::new("Le build vise ici :").color(Color32::GRAY).small());
                            let mut seen = Vec::new();
                            for w in wanted {
                                if !seen.contains(&w) {
                                    ui.label(RichText::new(format!("  {w}")).color(Color32::GRAY).small());
                                    seen.push(w);
                                }
                            }
                        }
                    }
                }
            }
            None => {
                ui.label(RichText::new("Ne se porte pas : pas de comparaison.").color(Color32::GRAY));
            }
        }
        ui.add_space(8.0);

        // Valeur marchande.
        let value = self.value_of(&it);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Valeur").color(Color32::GRAY));
            ui.label(RichText::new(value.label.text()).color(ui::value_color(value.label)).strong());
        });
        ui.label(RichText::new(&value.summary).color(Color32::LIGHT_GRAY));
        for r in &value.reasons {
            ui.label(RichText::new(format!("• {r}")).color(Color32::GRAY));
        }
        if trade::priceable(&it) {
            ui.add_space(6.0);
            self.market_section(ui, &it);
        }
        ui.add_space(8.0);
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
        // Message d'état affiché 30 s : un seul réveil pour l'effacer, pas de rafraîchissement continu.
        if let Some((_, at)) = &self.status {
            if let Some(left) = Duration::from_secs(30).checked_sub(at.elapsed()) {
                ctx.request_repaint_after(left);
            }
        }

        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(Color32::from_rgb(12, 11, 10)).inner_margin(egui::Margin { left: 12, right: 6, top: 6, bottom: 6 }))
            .show(ctx, |ui| {
            let missing = self.gear.untracked().len();
            ui.horizontal(|ui| {
                match &self.status {
                    Some((msg, at)) if at.elapsed().as_secs() < 30 => {
                        ui.label(RichText::new(msg).color(Color32::from_rgb(220, 200, 150)));
                    }
                    _ => {
                        ui.label(
                            RichText::new("Ctrl+C : diagnostiquer · Ctrl+Alt+C : suivre la pièce").color(Color32::GRAY),
                        );
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.settings_menu(ui);
                    ui.add_space(8.0);
                    self.update_status(ui);
                    if let Some(e) = &self.ui_art.error {
                        ui.label(RichText::new("fond du jeu indisponible").color(Color32::GRAY)).on_hover_text(e);
                    }
                    if !self.assets.ready() {
                        ui.label(RichText::new("chargement des illustrations…").color(Color32::GRAY));
                    }
                    if missing > 0 {
                        ui.label(RichText::new(format!("{missing} emplacement(s) non suivi(s)")).color(Color32::GRAY));
                    }
                });
            });
        });
        self.handle_dropped_files(ctx);
        self.sync_league(ctx);
        self.welcome_window(ctx);
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
            // Barre de défilement à part (pas par-dessus le contenu) et un peu d'air à droite.
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 }).show(ui, |ui| {
                    self.diagnostic(ui, comparison.as_ref())
                });
            });
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
        println!("{:#?}", compare::compare(&it, &gear, basis));
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("poe2-tracker")
            .with_inner_size([1180.0, 900.0])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("poe2-tracker", options, Box::new(move |cc| Ok(Box::new(App::new(cc, dirs, stored, config)))))
}
