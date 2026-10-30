//! poe2-tracker : suivi de l'équipement et diagnostic des objets pour Path of Exile 2.
//!
//! En jeu : Ctrl+C sur un objet pour le diagnostiquer, Ctrl+Alt+C pour l'ajouter à
//! l'équipement suivi.

// Sous Windows, pas de console noire à côté de la fenêtre.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod art;
mod atlas;
mod augments;
mod builds;
mod bundles;
mod capture;
mod changelog;
mod coach;
mod dat;
mod compare;
mod desktop;
mod gametext;
mod gear;
mod gem_info;
mod guide;
mod item;
mod leagues;
mod logwatch;
mod mapping;
mod passive_icons;
mod richtext;
mod paths;
mod prices;
mod route;
mod rules;
mod stats;
mod statdesc;
mod trade;
mod ui;
mod ui_art;
mod uniques;
mod updater;
mod vendor_regex;
mod views;
mod weights;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use regex::Regex;
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
    /// Valeur à partir de laquelle un objet mérite d'aller au marché, dans la devise
    /// `market_min_unit` (le nom date du temps où c'était toujours des exalts).
    market_min_ex: f64,
    /// Devise du seuil : « ex », « chaos » ou « div ».
    market_min_unit: String,
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
    /// Onglet Boss (expérimental) : les boss d'endgame affrontés.
    boss_tab: bool,
    /// Langue de l'interface (« fr », « en » ; vide = celle du système).
    lang: String,
}

impl Default for Config {
    fn default() -> Self {
        Config { league: String::new(), game_dir: PathBuf::new(), market_min_ex: 10.0, log_path: PathBuf::new(), welcomed: false, seen_version: String::new(),
            auto_update: true,
            trade_prices: true,
            auto_estimate: true,
            open_on_copy: true,
            boss_tab: false,
            lang: String::new(),
            market_min_unit: "ex".into(),
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
    socket_tips: HashMap<(String, String, u32), Vec<(String, String)>>,
    /// Catalogue d'augments pour lequel ces caches valent.
    catalog_id: usize,
    /// Runes recommandées du build actif (identifiant du build, étapes).
    build_runes: Option<(String, Vec<builds::StepRunes>)>,
    /// Rareté des pièces recommandées (Mobalytics), par build.
    build_rarities: Option<(String, Vec<builds::StepRarities>)>,
    /// Joyaux de l'arbre (Mobalytics), par build.
    build_jewels: Option<(String, Vec<builds::StepJewels>)>,
    /// Textes de l'auteur (Mobalytics), par build.
    build_guide: Option<(String, Option<builds::BuildGuide>)>,
    /// Choix de récompenses de quête du build actif.
    build_quests: Option<(String, Vec<builds::QuestChoice>)>,
    /// Fenêtre des nouveautés ouverte : versions à montrer.
    changelog: Option<Vec<changelog::Release>>,
    /// Étape regardée en aperçu (autre que celle de la progression), non enregistrée.
    step_preview: Option<String>,
    /// Seuil du marché et sa devise (voir `Config`).
    market_min_ex: f64,
    market_min_unit: String,
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
    /// Case d'équipement dont le détail est ouvert (clic sur la case ou son étoile).
    slot_detail: Option<Slot>,
    /// Bandeaux « comparer » du diagnostic affiché (au-dessus de l'estimation).
    diag_bands: Option<DiagBands>,
    /// Ce que montre ce détail : la pièce portée face au build, ou la pièce diagnostiquée face au
    /// build ou à la pièce portée.
    detail_mode: DetailMode,
    /// Récompense modifiable de la route dont on change le choix (petite modale).
    route_pick: Option<String>,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    /// Arbre de l'Atlas lu dans les fichiers du jeu (au premier affichage de l'onglet).
    atlas_tree: Arc<Mutex<Option<Result<atlas::AtlasTree, String>>>>,
    atlas_loading: bool,
    atlas_view: atlas::AtlasView,
    /// Personnage dont l'équipement, l'historique et le build sont affichés.
    character: Option<String>,
    /// Hauteur réelle de la barre personnage / build (mesurée à l'affichage).
    bar_h: f32,
    tab: views::Tab,
    /// Talents ou Atlas sur toute la fenêtre, sans les panneaux autour.
    full_view: bool,
    /// Onglet du panneau de droite.
    side_tab: views::SideTab,
    /// Bloc en bas à gauche : maps (vrai) ou route de leveling. `None` : les maps dès que le
    /// personnage en a fait une.
    bottom_maps: Option<bool>,
    /// Maps affichées dans l'onglet Maps : toutes, solo (`Some(false)`) ou en groupe.
    maps_filter: Option<bool>,
    /// Tier affiché dans l'onglet Maps : niveau de zone, 0 pour les arènes de boss, tous sinon.
    maps_tier: Option<u32>,
    /// Détail de l'estimation ouvert, pour l'objet de ce texte (troisième niveau : historique,
    /// diagnostic, estimation). Sans effet quand un autre objet est diagnostiqué.
    estimate_open: Option<String>,
    /// Onglet Marché : catégorie poe.ninja affichée et recherche.
    market_category: String,
    market_search: String,
    /// Ligue regardée dans l'onglet Marché quand ce n'est pas celle du personnage, et ses cours.
    market_league: Option<(String, prices::Prices)>,
    /// Onglet Marché : objet dont le détail est ouvert (par son nom).
    market_open: Option<String>,
    /// Historiques des cours (« ligue|identifiant poe.ninja ») : `None` pendant le chargement.
    market_history: Arc<Mutex<HashMap<String, Option<Result<Vec<(String, f64)>, String>>>>>,
    /// Période de la courbe du détail : 7 jours, 30 jours, tout.
    market_period: usize,
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
    market_min_unit: String,
    boss_tab: bool,
    lang: i18n::Lang,
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
            market_min_unit: stored.market_min_unit.clone(),
            boss_tab: stored.boss_tab,
            lang: i18n::base(),
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
/// Nom d'une map : celui affiché par le jeu, sinon son code sans « Map ».
fn run_name(r: &mapping::Run) -> String {
    r.zone.as_deref().map(|z| gametext::name(z).into_owned()).unwrap_or_else(|| r.code.trim_start_matches("Map").to_string())
}

/// Tier du waystone d'après le niveau de zone (65 = T1… 80 = T16), « boss » pour une arène.
fn run_tier(r: &mapping::Run) -> String {
    match r.kind {
        mapping::RunKind::Boss => tr!("boss").into(),
        mapping::RunKind::Map if r.level > 80 => tr!("T16+ · niv {level}", level = r.level),
        mapping::RunKind::Map if r.level >= 65 => format!("T{}", r.level - 64),
        mapping::RunKind::Map => tr!("niv {level}", level = r.level),
    }
}

/// Tier d'une map : l'image du waystone du jeu (T1 à T16) ou le crâne des boss, le texte au
/// survol. Le texte à défaut d'image (niveau hors tiers, image pas encore extraite).
fn tier_badge(ui: &mut egui::Ui, icons: &mut passive_icons::PassiveIcons, r: &mapping::Run, size: f32, color: Color32) {
    tier_image(ui, icons, r, size, color).on_hover_text(run_tier(r));
}

/// L'image du tier seule, sans infobulle (la ligne d'historique a la sienne).
fn tier_image(ui: &mut egui::Ui, icons: &mut passive_icons::PassiveIcons, r: &mapping::Run, size: f32, color: Color32) -> egui::Response {
    let tier = r.level.checked_sub(64).filter(|t| (1..=16).contains(t) && r.kind == mapping::RunKind::Map);
    let icon = match (r.kind, tier) {
        // Arène de boss : le crâne des boss de l'Atlas.
        (mapping::RunKind::Boss, _) => Some("art/textures/interface/2d/2dart/uiimages/ingame/atlasscreen/atlasiconcontent/atlasiconcontentmapbossspecial.dds".to_string()),
        (_, Some(t)) => Some(format!("art/2ditems/maps/endgamemaps/endgamemap{t}.dds")),
        _ => None,
    };
    let texture = icon.and_then(|i| icons.get(&i));
    match texture {
        Some(tex) => {
            let resp = ui.add(egui::Image::new((tex.id(), Vec2::splat(size))));
            // Le crâne rond des boss, cerclé de blanc pour se détacher des waystones.
            if r.kind == mapping::RunKind::Boss {
                ui.painter().circle_stroke(resp.rect.center(), size / 2.0 - 0.5, egui::Stroke::new(1.5_f32, Color32::from_gray(235)));
            }
            resp
        }
        None => ui.label(RichText::new(run_tier(r)).color(color).size(13.5)),
    }
}

const TOMB_ICON: &str = "art/textures/interface/2d/2dart/uiimages/ingame/partymenu/4k/partyrevivesicon.dds";
const PORTAL_ICON: &str = "art/textures/interface/2d/2dart/uiimages/ingame/4k/worldpaneltownportalicon.dds";

/// Image du jeu suivie d'un nombre (morts, retours en ville).
/// Le nombre prend `color` quand il n'est pas nul (rouge pour les morts, bleu pour les retours).
fn icon_count(ui: &mut egui::Ui, icons: &mut passive_icons::PassiveIcons, icon: &str, n: u32, text: &str, color: Color32) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        match icons.get(icon) {
            Some(tex) => ui.add(egui::Image::new((tex.id(), Vec2::splat(22.0)))),
            None => ui.label(RichText::new(text).color(Color32::GRAY).size(13.5)),
        };
        ui.label(RichText::new(n.to_string()).color(if n > 0 { color } else { Color32::GRAY }).size(15.0));
    });
}

/// Ce que mesure le temps d'une map, au survol du chrono et des durées. Sans serveur pour
/// synchroniser les apps d'un groupe, chacun ne voit que sa propre présence.
fn time_note(r: &mapping::Run) -> &'static str {
    if r.group {
        tr!("En groupe : c'est ton propre temps passé dans la map (hors repaire et AFK), pas la durée de la map pour tout le groupe.")
    } else {
        tr!("Temps passé dans la map, hors repaire et AFK.")
    }
}

/// « solo » ou « en groupe », à la suite du nom de la map.
fn group_label(r: &mapping::Run) -> RichText {
    RichText::new(if r.group { tr!("en groupe") } else { tr!("solo") }).color(Color32::GRAY).size(13.5)
}

/// En-tête et mods d'un waystone, en petit.
fn waystone_lines(ui: &mut egui::Ui, w: &mapping::Waystone) {
    if !w.stats.is_empty() {
        ui.label(RichText::new(w.stats.join(" · ")).color(Color32::LIGHT_GRAY).size(13.5));
    }
    for m in &w.mods {
        ui.label(RichText::new(gametext::line(m)).color(Color32::from_rgb(140, 150, 230)).size(13.5));
    }
}

/// Mécanique d'une arène de boss d'après son code (noms du Marché).
fn boss_mechanic(code: &str) -> &'static str {
    [("Ritual", tkey!("Rituel")), ("Delirium", tkey!("Délire")), ("Abyss", tkey!("Abîme")), ("Expedition", tkey!("Expédition")), ("Chayula", tkey!("Brèche")), ("Breach", tkey!("Brèche"))]
        .iter()
        .find(|(k, _)| code.contains(k))
        .map_or(tr!("boss de l'Atlas"), |(_, v)| i18n::tr(v))
}

/// Date et heure d'une map (heure du log) : « 29 oct. 02:37 » (« Oct 29 02:37 » en anglais).
fn date_text(t: i64) -> String {
    const MONTHS: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
    const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // Jour civil depuis le nombre de jours depuis 1970 (calendrier grégorien).
    let z = t.div_euclid(86_400) + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let secs = t.rem_euclid(86_400);
    let time = format!("{:02}:{:02}", secs / 3600, secs / 60 % 60);
    if i18n::english() {
        format!("{} {day} {time}", MONTHS_EN[month as usize - 1])
    } else {
        format!("{day} {} {time}", MONTHS[month as usize - 1])
    }
}

/// Chronomètre : « 4:07 », « 1:02:45 ».
fn clock_text(secs: i64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Durée arrondie : « 45 s », « 12 min », « 1 h 05 ».
fn duration_text(secs: i64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        // Les secondes comptent sur les maps rapides.
        s if s < 3600 => format!("{} min {:02} s", s / 60, s % 60),
        s => format!("{} h {:02}", s / 3600, (s + 30) / 60 % 60),
    }
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
        Some(gear::Kind::TwoHanded) => tr!("Arme à deux mains").to_string(),
        Some(gear::Kind::Ring) => tr!("Bague").to_string(),
        Some(gear::Kind::Charm) => tr!("Charme").to_string(),
        None => gametext::name(&it.item_class).into_owned(),
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
        let whole = !c.full_scan_done() || !c.route_scan_done() || !c.maps_scan_done() || !c.picks_scan_done();
        if whole {
            c.begin_replay();
        }
        let events = if whole { logwatch::read_all(&log_path) } else { logwatch::read_recent(&log_path, 8 * 1024 * 1024) };
        for ev in events {
            c.handle(&ev);
        }
        c.mark_full_scan();
        c.mark_route_scan();
        c.mark_maps_scan();
        c.mark_picks_scan();
        c.clock = logwatch::log_clock_offset(&log_path);
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
        // Langue de l'interface avant tout texte (nouveautés, fil d'événements rejoué).
        i18n::set(i18n::Lang::from_code(&stored.lang).unwrap_or_else(i18n::Lang::system));
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
        gametext::start(cc.egui_ctx.clone(), dirs.cache_dir().to_path_buf(), config.game_dir.clone());
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
            build_rarities: None,
            build_jewels: None,
            build_guide: None,
            build_quests: None,
            step_preview: None,
            market_min_ex: config.market_min_ex,
            market_min_unit: config.market_min_unit.clone(),
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
            // Développement : `POE2_TRACKER_SLOT=Body` ouvre le détail d'une case au démarrage.
            route_pick: None,
            // Développement : modale du diagnostic (POE2_TRACKER_DETAIL=worn|build).
            detail_mode: match std::env::var("POE2_TRACKER_DETAIL").as_deref() {
                Ok("worn") => DetailMode::DiagWorn,
                Ok("build") => DetailMode::DiagBuild,
                _ => DetailMode::Worn,
            },
            diag_bands: None,
            slot_detail: std::env::var("POE2_TRACKER_SLOT").ok().and_then(|v| Slot::ALL.into_iter().find(|s| format!("{s:?}") == v)),
            data_dir: dirs.data_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
            atlas_tree: Arc::new(Mutex::new(None)),
            atlas_loading: false,
            atlas_view: atlas::AtlasView::default(),
            character: None,
            bar_h: 34.0,
            // Développement : onglet de droite (POE2_TRACKER_SIDE=market).
            market_category: "Currency".into(),
            market_search: String::new(),
            market_league: None,
            // Développement : détail d'un objet du marché ouvert (POE2_TRACKER_MARKET=nom).
            market_open: std::env::var("POE2_TRACKER_MARKET").ok(),
            market_history: Arc::default(),
            market_period: 1,
            estimate_open: None,
            // Développement : bloc du bas sur la route (POE2_TRACKER_BOTTOM=route).
            bottom_maps: (std::env::var("POE2_TRACKER_BOTTOM").as_deref() == Ok("route")).then_some(false),
            maps_filter: None,
            maps_tier: None,
            side_tab: match std::env::var("POE2_TRACKER_SIDE").as_deref() {
                Ok("market") => views::SideTab::Market,
                Ok("maps") => views::SideTab::Maps,
                Ok("bosses") => views::SideTab::Bosses,
                _ => views::SideTab::Diagnostic,
            },
            tab: match std::env::var("POE2_TRACKER_TAB").as_deref() {
                Ok("carac") => views::Tab::Stats,
                Ok("sorts") => views::Tab::Skills,
                Ok("talents") => views::Tab::Passives,
                Ok("atlas") => views::Tab::Atlas,
                Ok("guide") => views::Tab::Guide,
                _ => views::Tab::Equipment,
            },
            // Développement : `POE2_TRACKER_FULL=1` ouvre la vue en plein écran.
            full_view: std::env::var_os("POE2_TRACKER_FULL").is_some(),
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
        gametext::start(ctx.clone(), cache.clone(), self.config.game_dir.clone());
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
        let title = if first { tr!("Bienvenue dans poe2-tracker") } else { tr!("Dossier du jeu") };
        // Premier lancement : on ne ferme qu'avec « Commencer ».
        let (_, closed) = ui::modal(ctx, "welcome", title, !first, 600.0, |ui| {
                if first {
                    ui.label(tr!("poe2-tracker accompagne ta partie sans jamais toucher au jeu : il lit seulement son journal et ce que tu copies avec Ctrl+C."));
                    ui.add_space(8.0);
                }
                ui.label(RichText::new(tr!("Dossier d'installation de Path of Exile 2")).color(ui::GOLD).strong());
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.welcome.path).desired_width(380.0).hint_text(tr!("dossier qui contient « logs »")));
                    if ui.add_enabled(self.welcome.picking.is_none(), egui::Button::new(tr!("Parcourir…"))).clicked() {
                        let (tx, rx) = channel();
                        let ctx = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(rfd::FileDialog::new().set_title(tr!("Dossier de Path of Exile 2")).pick_folder());
                            ctx.request_repaint();
                        });
                        self.welcome.picking = Some(rx);
                    }
                    if ui.button(tr!("Rechercher")).clicked() {
                        match paths::detect() {
                            Some(d) => {
                                self.welcome.path = d.display().to_string();
                                self.welcome.message = None;
                            }
                            None => self.welcome.message = Some(tr!("Introuvable aux emplacements habituels (Steam, version autonome) : indique-le avec « Parcourir ».").into()),
                        }
                    }
                });
                let line = |ui: &mut egui::Ui, ok: bool, what: &str, missing: &str| {
                    let (mark, color) = if ok { ("✔", Color32::from_rgb(120, 200, 120)) } else { ("✘", Color32::from_rgb(230, 110, 90)) };
                    ui.label(RichText::new(format!("{mark}  {what}{}", if ok { String::new() } else { format!(" — {missing}") })).color(color));
                };
                line(ui, check.log, tr!("Journal du jeu (logs/Client.txt)"), tr!("pas de personnage, de route de leveling ni de fil d'événements"));
                line(ui, check.assets, tr!("Images du jeu (Bundles2 ou Content.ggpk)"), tr!("pas de fond ni d'icônes de l'arbre"));
                if !check.log && !check.assets {
                    ui.label(RichText::new(tr!("Jeu introuvable ici. Le diagnostic des objets (Ctrl+C) marche quand même.")).color(Color32::from_rgb(230, 110, 90)));
                }
                if let Some(m) = &self.welcome.message {
                    ui.label(RichText::new(m).color(Color32::GRAY));
                }
                if first {
                    ui.add_space(10.0);
                    ui.label(RichText::new(tr!("En jeu")).color(ui::GOLD).strong());
                    ui.label(tr!("•  Ctrl+C sur un objet : diagnostic (comparaison avec ta pièce, valeur au marché, verdict)."));
                    ui.label(tr!("•  Ctrl+Alt+C sur une pièce que tu portes : elle rejoint l'équipement suivi, base des comparaisons."));
                    ui.label(tr!("•  En bas : la route de leveling (points de passif, résistances…), cochée toute seule au fil de la campagne."));
                    ui.add_space(10.0);
                    ui.label(RichText::new(tr!("Build (facultatif)")).color(ui::GOLD).strong());
                    ui.label(tr!("Sans build, le diagnostic compare simplement à tes pièces. Avec un build (lien Kami-labs ou fichier .build du planificateur), les verdicts suivent ses objectifs et les onglets Aptitudes et Talents (ascendance comprise) s'ouvrent."));
                    if ui.button(tr!("Ajouter un build…")).clicked() {
                        self.add_build.open = true;
                        self.add_build.message = None;
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(if first { tr!("Commencer") } else { tr!("Valider") }).color(ui::GOLD).strong()).clicked() {
                            done = true;
                        }
                        if !first && ui.button(tr!("Annuler")).clicked() {
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

    /// Rang de l'étape choisie dans la barre du haut (celle de la progression, ou l'aperçu) :
    /// celle que montrent les onglets du build.
    fn shown_step_index(&self) -> Option<usize> {
        let c = self.coach.as_ref()?;
        let label = self.judging_step().map(|s| s.label.clone());
        label.and_then(|l| c.steps.iter().position(|s| s.label == l)).or_else(|| c.current_step_index()).filter(|i| *i < c.steps.len())
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
    fn socket_tips(&mut self, it: &Item, slot: Slot) -> Vec<(String, String)> {
        let (Some(catalog), Some((_, step))) = (self.augments.get(), self.basis()) else { return vec![] };
        let level = self.coach.as_ref().map(|c| c.level()).unwrap_or(100);
        let key = (it.raw.clone(), step.label.clone(), level);
        if let Some(t) = self.socket_tips.get(&key) {
            return t.clone();
        }
        let prices = self.prices.data.lock().unwrap();
        // Nom, puis effets et prix.
        let tips: Vec<(String, String)> = augments::suggestions(it, &catalog.augments, level, |v| compare::build_value(&step, &[slot], v))
            .into_iter()
            .map(|(a, e)| {
                let price = prices.as_ref().and_then(|p| Some(format!(" ≈ {}", p.fmt(p.currency(&a.name)?)))).unwrap_or_default();
                (a.name.clone(), format!(" : {}{price}", e.text.join(", ")))
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

    /// Joyaux que le build pose dans l'arbre à l'étape `label` (Mobalytics), mods traduits.
    fn tree_jewels(&mut self, label: &str) -> Vec<views::TreeJewel> {
        let Some(id) = self.builds.active().map(|b| b.id.clone()) else { return vec![] };
        if self.build_jewels.as_ref().is_none_or(|(b, _)| *b != id) {
            self.build_jewels = Some((id.clone(), builds::load_jewels(&self.builds.dir_of(&id))));
        }
        let catalog = self.augments.get();
        let steps = &self.build_jewels.as_ref().unwrap().1;
        let Some(step) = steps.iter().find(|s| s.name == label) else { return vec![] };
        step.jewels
            .iter()
            .map(|j| views::TreeJewel {
                node: j.node,
                icon: j.icon.clone(),
                name: j.name.clone(),
                base: j.base.clone(),
                color: ui::rarity_color(j.rarity),
                // Unique : textes déjà résolus à l'import ; sinon les mods traduits par RePoE.
                lines: if j.texts.is_empty() {
                    j.mods.iter().map(|m| catalog.as_ref().and_then(|c| c.mod_texts.get(m).cloned()).unwrap_or_else(|| m.clone())).collect()
                } else {
                    j.texts.clone()
                },
            })
            .collect()
    }

    /// Ce que la campagne a déjà donné au personnage (récompenses cochées sur la route).
    fn quest_bonuses(&self) -> route::Bonuses {
        self.coach.as_ref().and_then(|c| c.character()).map(|(_, ch)| route::bonuses(&ch.done, &ch.picks)).unwrap_or_default()
    }

    /// Choix de récompenses de quête que le build recommande (Mobalytics).
    fn quest_choices(&mut self) -> Vec<builds::QuestChoice> {
        let Some(id) = self.builds.active().map(|b| b.id.clone()) else { return vec![] };
        if self.build_quests.as_ref().is_none_or(|(b, _)| *b != id) {
            self.build_quests = Some((id.clone(), builds::load_quest_choices(&self.builds.dir_of(&id))));
        }
        self.build_quests.as_ref().unwrap().1.clone()
    }

    /// Runes avec leur icône et leur prix (« Greater Iron Rune ≈ 3 ex »), une ligne par exemplaire.
    /// `class` : classe de la pièce qui les porte (« Staves »), pour n'en garder que l'effet utile.
    fn rune_list(&mut self, runes: &[String], class: Option<&str>) -> Vec<ui::RuneLine> {
        let priced: Vec<(String, String)> = {
            let prices = self.prices.data.lock().unwrap();
            runes.iter().map(|n| (n.clone(), prices.as_ref().and_then(|p| Some(format!(" ≈ {}", p.fmt(p.currency(n)?)))).unwrap_or_default())).collect()
        };
        priced.into_iter().map(|(name, rest)| self.rune_line(name, rest, class)).collect()
    }

    /// Ligne d'une rune (ou d'un augment) : icône, nom, suite, et de quoi remplir sa fiche. Avec la
    /// classe de la pièce qui la porte, la fiche ne montre que l'effet sur ce type d'objet.
    fn rune_line(&mut self, name: String, rest: String, class: Option<&str>) -> ui::RuneLine {
        let (kind, effects) = match self.augments.get().and_then(|c| c.augments.iter().find(|a| a.name == name).cloned()) {
            Some(a) => {
                let kind = match a.kind.as_str() {
                    "Rune" => tr!("Rune"),
                    "SoulCore" => tr!("Noyau d'âme"),
                    "Idol" => tr!("Idole"),
                    k => k,
                };
                let kind = if a.level > 0 { tr!("{kind} · niveau {level}", kind, level = a.level) } else { kind.to_string() };
                let effects = match class.and_then(|c| a.effect_for(c)) {
                    Some(e) => vec![(e.category.clone(), e.text.clone())],
                    None => a.effects.iter().map(|e| (e.category.clone(), e.text.clone())).collect(),
                };
                (kind, effects)
            }
            None => (String::new(), vec![]),
        };
        ui::RuneLine { image: self.assets.texture_by_name(&name), mark: None, name, rest, color: ui::GOLD, kind, effects }
    }

    /// Colonne « recommandé par le build » de la fiche d'une pièce portée en `slot`.
    /// `it` : la pièce portée, comparée ligne par ligne ; `None` pour une case vide.
    fn build_card(&mut self, it: Option<&Item>, slot: Slot) -> Option<ui::BuildCard> {
        let (_, step) = self.basis()?;
        let base = step.base_for(slot).to_string();
        let unique = self.recommended_unique(&step, slot);
        let catalog = self.augments.get();
        let lines = match it {
            Some(it) => compare::check_targets(it, &step, slot, |b| catalog.as_ref()?.defences.get(b).copied()),
            None => step.targets_for(slot).map(|t| (t.line.clone(), compare::Have::Unknown)).collect(),
        };
        let runes = self.recommended_runes(slot);
        let base_defences = catalog.as_ref().and_then(|c| c.defences.get(&base).copied());
        let mut implicits = catalog.as_ref().and_then(|c| c.implicits.get(&base).cloned()).unwrap_or_default();
        // Compétence accordée par la base (bâton, baguette…), comme sur la fiche de la pièce.
        if let (Some(skills), Some(coach)) = (catalog.as_ref().and_then(|c| c.skills.get(&base)), self.coach.as_ref()) {
            implicits.extend(skills.iter().map(|id| format!("{}{}", tr!("Octroie l'Aptitude : "), coach.catalog().gem(id))));
        }
        let image = match &unique {
            Some(u) => self.assets.texture_unique(u),
            None if !base.is_empty() => self.assets.texture_by_name(&base),
            None => None,
        };
        // Classe de la pièce visée (sa base), à défaut de celle portée : l'effet utile des runes.
        let class = catalog.as_ref().and_then(|c| c.classes.get(&base)).map(|(c, _)| trade::text_class(c)).or_else(|| it.map(|it| it.item_class.clone()));
        let runes = self.rune_list(&runes, class.as_deref());
        let card = ui::BuildCard { step: step.label.clone(), base, base_defences, implicits, unique, lines, runes,
            image,
        };
        (!card.is_empty()).then_some(card)
    }

    /// Pièce rare que vise le build sur `slot` (type de base et mods visés), écrite comme un
    /// texte d'objet du jeu pour en estimer le prix au marché.
    /// Rareté de la pièce que le build vise sur `slot` : celle de Mobalytics quand on la connaît,
    /// sinon d'après le nombre de mods visés (fioles et charmes : magiques jusqu'à deux).
    fn recommended_rarity(&mut self, step: &guide::Step, slot: Slot) -> item::Rarity {
        if let Some(id) = self.builds.active().map(|b| b.id.clone()) {
            if self.build_rarities.as_ref().is_none_or(|(b, _)| *b != id) {
                self.build_rarities = Some((id.clone(), builds::load_rarities(&self.builds.dir_of(&id))));
            }
            let known = self.build_rarities.as_ref().unwrap().1.iter().find(|s| s.name == step.label).and_then(|s| s.items.get(&slot).copied());
            if let Some(r) = known {
                return r;
            }
        }
        let lines = step.targets_for(slot).count();
        let small = matches!(slot, Slot::LifeFlask | Slot::ManaFlask) || gear::CHARMS.contains(&slot);
        match lines {
            0 => item::Rarity::Normal,
            1 | 2 if small => item::Rarity::Magic,
            _ => item::Rarity::Rare,
        }
    }

    fn recommended_rare(&self, step: &guide::Step, slot: Slot, rarity: item::Rarity) -> Option<Item> {
        let (_, base, targets) = step.targets.iter().chain(&step.side_targets).find(|(s, b, _)| *s == slot && !b.is_empty())?;
        let class = self.augments.get()?.classes.get(base)?.0.clone();
        let mods: Vec<&str> = targets.iter().map(|t| t.line.as_str()).collect();
        // Fioles et charmes sont au plus magiques : une ligne de nom, celui de la base.
        // Un objet normal ou magique n'a qu'une ligne de nom (la base) ; un rare en a deux.
        let head = match rarity {
            item::Rarity::Rare => format!("Rare\nRecommandation du build\n{base}"),
            item::Rarity::Magic => format!("Magic\n{base}"),
            _ => format!("Normal\n{base}"),
        };
        let text = format!(
            "Item Class: {}\nRarity: {head}\n--------\nItem Level: 1\n--------\n{}",
            trade::text_class(&class),
            mods.join("\n")
        );
        item::parse(&text)
    }

    /// Détail d'une case d'équipement, posé sur la zone d'équipement (le reste de la fenêtre
    /// reste utilisable) : la pièce portée face à ce que recommande le build, ligne à ligne
    /// pour comparer d'un coup d'œil, puis le prix de la recommandation et sa recherche.
    /// `k_min`, `k_max` : agrandissement, entre les deux selon la place (celles du diagnostic, sur
    /// toute la fenêtre, ont une taille fixe).
    fn slot_detail(&mut self, ui: &mut egui::Ui, zone: egui::Rect, k_min: f32, k_max: f32) {
        let Some(slot) = self.slot_detail else { return };
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.slot_detail = None;
            return;
        }
        let mode = self.detail_mode;
        // La pièce de gauche : la pièce portée, ou celle du diagnostic.
        let item = match mode {
            DetailMode::Worn => self.gear.item(slot),
            _ => self.diagnosed.clone(),
        };
        let card = self.build_card(item.as_ref(), slot);
        let step = self.basis().map(|(_, s)| s);
        // Face à la pièce portée : son bloc (en-tête, écarts, runes, score), comparé à la pièce
        // diagnostiquée.
        let worn_side = if mode == DetailMode::DiagWorn {
            item.clone().map(|it| {
                let comparison = self.compare_at(&it, Some(slot));
                let worn_item = self.gear.item(slot);
                let worn_score = worn_item.as_ref().and_then(|w| {
                    let c = self.build_card(Some(w), slot);
                    self.worn_view(slot, w, c.as_ref()).score
                });
                self.diag_sides(&it, comparison.as_ref(), card.as_ref(), worn_item.as_ref(), &None, None, worn_score, None).0
            })
        } else {
            None
        };
        let unique = step.as_ref().and_then(|s| self.recommended_unique(s, slot));
        let rec_rarity = step.as_ref().map_or(item::Rarity::Rare, |s| self.recommended_rarity(s, slot));
        let rare = if unique.is_none() { step.as_ref().and_then(|s| self.recommended_rare(s, slot, rec_rarity)) } else { None };
        // L'estimation de la pièce rare recommandée part à l'ouverture (si le trade est permis).
        if let Some(r) = rare.as_ref().filter(|_| mode != DetailMode::DiagWorn) {
            if self.stored.trade_prices && trade::priceable(r) && self.trade.get(r).is_none() {
                self.trade.request(r);
            }
        }
        let league = self.league.clone();
        let green = Color32::from_rgb(225, 220, 210);
        // Prix et recherche de la recommandation : texte, couleur, lien, et s'il est en cours.
        let mut price: Option<(String, Color32)> = None;
        let mut basis: Option<String> = None;
        let mut link: Option<(String, &str)> = None;
        let mut pending = false;
        if let Some(u) = &unique {
            let found = {
                let data = self.prices.data.lock().unwrap();
                data.as_ref().and_then(|p| p.unique_by_name(u).map(|(b, div, n)| (b.to_string(), p.to_ex(div), n)))
            };
            match found {
                Some((base, ex, n)) => {
                    price = Some((format!("≈ {}", self.fmt_price(ex)), green));
                    basis = Some(if n < prices::MIN_LISTINGS { tr!("{n} annonces seulement, cours de poe.ninja", n) } else { trn!(n, "{n} annonce, cours de poe.ninja", "{n} annonces, cours de poe.ninja") });
                    link = Some((uniques::trade_url(&league, u, Some(&base)), tr!("Chercher sur le trade ↗")));
                }
                None => {
                    price = Some((tr!("pas de prix au marché").into(), Color32::GRAY));
                    link = Some((uniques::trade_url(&league, u, None), tr!("Chercher sur le trade ↗")));
                }
            }
        } else if let Some(r) = &rare {
            let fallback = (uniques::base_trade_url(&league, &r.base), tr!("Chercher la base sur le trade ↗"));
            match self.trade.get(r) {
                Some(e @ trade::Estimate::Done { .. }) => {
                    let trade::Estimate::Done { total, url, relaxed, .. } = &e else { unreachable!() };
                    price = Some(match e.value_ex() {
                        Some(ex) => (format!("≈ {}", self.fmt_price(ex)), ui::confidence_color(e.confidence_pct())),
                        None => (tr!("pas assez d'annonces pour estimer").into(), Color32::GRAY),
                    });
                    let mut b = trn!(*total, "{n} annonce comparable", "{n} annonces comparables");
                    if *relaxed > 0 {
                        b.push_str(&trn!(*relaxed, " · recherche élargie ({n} mod en moins)", " · recherche élargie ({n} mods en moins)"));
                    }
                    basis = Some(b);
                    link = Some((url.clone(), tr!("Voir les annonces sur le trade ↗")));
                }
                Some(trade::Estimate::Pending(msg)) => {
                    pending = true;
                    price = Some((msg, Color32::LIGHT_GRAY));
                    link = Some(fallback);
                }
                Some(trade::Estimate::Failed(err)) => {
                    price = Some((err, Color32::from_rgb(230, 120, 100)));
                    link = Some(fallback);
                }
                None => {
                    if !self.stored.trade_prices {
                        price = Some((tr!("estimation sur le trade coupée dans les réglages").into(), Color32::GRAY));
                    }
                    link = Some(fallback);
                }
            }
        }
        let title = match (&step, mode) {
            (_, DetailMode::DiagWorn) => tr!("{place} · comparé à ta pièce", place = slot.place()),
            (Some(s), _) => format!("{} · {}", slot.place(), guide::step_text(&s.label)),
            (None, _) => slot.place(),
        };
        let left_title = if mode == DetailMode::Worn { tr!("ACTUELLEMENT ÉQUIPÉ") } else { tr!("PIÈCE DIAGNOSTIQUÉE") };
        let right_title = if mode == DetailMode::DiagWorn { tr!("ACTUELLEMENT ÉQUIPÉ") } else { tr!("RECOMMANDÉ PAR LE BUILD") };

        // Modale sur la zone d'équipement (fond assombri, un clic à côté la ferme), ajustée à son
        // contenu : hauteur mesurée au dernier affichage, largeur de chaque colonne d'après ses
        // lignes ; réduite à la place libre (le contenu défile alors).
        let height_id = ui.id().with("slot_detail_h");
        let natural: f32 = ui.data(|d| d.get_temp(height_id)).unwrap_or(400.0);
        const MODAL_MARGIN: f32 = 24.0;
        ui.painter_at(zone).rect_filled(zone, 0.0, Color32::from_black_alpha(170));
        let backdrop = ui.interact(zone, ui.id().with("slot_detail_backdrop"), egui::Sense::click());
        // Colonne de la pièce équipée, la même que dans l'infobulle de la case.
        let worn = item.as_ref().map(|it| self.worn_view(slot, it, card.as_ref()));
        let build_def = card.as_ref().and_then(|b| b.base_defences).map(|d| defence_parts(d, [false; 3])).unwrap_or_default();
        // Unique recommandé : son texte (PoB), base, implicites et mods avec leurs plages.
        let unique_text = unique.as_ref().and_then(|u| self.coach.as_ref()?.catalog().uniques.get(u).cloned());
        let build_impl: Vec<String> = match &unique_text {
            Some(t) => t.implicits.clone(),
            None => card.as_ref().map(|b| b.implicits.clone()).unwrap_or_default(),
        };
        let unique_base = unique_text.as_ref().map(|t| t.base.clone()).or_else(|| card.as_ref().map(|b| b.base.clone())).filter(|b| !b.is_empty());
        // Base visée, valeurs ajustées par les mods visés (quantité, durée, charges).
        let build_flask = match (&card, self.augments.get()) {
            (Some(b), Some(c)) => {
                let base = unique_base.clone().unwrap_or_else(|| b.base.clone());
                c.flasks.get(&base).map(|f| {
                    let increase = |tpls: &[&str]| -> Option<f64> {
                        let total: f64 = b.lines.iter().map(|(l, _)| trade::template(l)).filter(|(t, _)| tpls.contains(&t.as_str())).filter_map(|(t, v)| v.map(|v| if t.contains("reduced") { -v } else { v })).sum();
                        (total != 0.0).then_some(total)
                    };
                    let scaled = |base: f64, tpls: &[&str]| -> (f64, bool) {
                        match increase(tpls) {
                            Some(p) => ((base * (1.0 + p / 100.0)).round(), true),
                            None => (base, false),
                        }
                    };
                    let duration = scaled(f.duration / 10.0, &["#% increased Duration", "#% reduced Duration"]);
                    flask_parts(
                        scaled(f.life.max(f.mana), &["#% increased Amount Recovered", "#% reduced Amount Recovered"]),
                        f.mana > 0.0,
                        duration,
                        duration,
                        scaled(f.charges_per_use, &["#% increased Charges per use", "#% reduced Charges per use"]),
                        scaled(f.charges_max, &["#% increased Charges", "#% reduced Charges"]),
                    )
                }).unwrap_or_default()
            }
            _ => vec![],
        };
        // Niveau requis de la base visée, sous les autres propriétés.
        let mut build_flask = build_flask;
        let build_base = unique_base.clone().or_else(|| card.as_ref().map(|b| b.base.clone())).unwrap_or_default();
        if let Some(level) = self.augments.get().and_then(|c| c.req_levels.get(&build_base).copied()) {
            build_flask.push(ui::req_level_parts(level, self.char_level()));
        }
        // Effet d'un charme pendant son utilisation (celui de la pièce est déjà dans ses mods).
        let build_buffs: Vec<String> = self.coach.as_ref().and_then(|c| c.catalog().charm_buffs.get(&build_base).cloned()).unwrap_or_default();
        // Comparaison côté recommandation : ce que vise le build, et si la pièce équipée l'a.
        let good = ("✓", ui::SOCKET_GOOD);
        let partial = ("~", ui::EMPTY_SOCKET);
        let missing = ("✗", ui::SOCKET_WRONG);
        // Défenses de la base visée face à celles de la pièce (sans les mods : un repère).
        let build_def_mark = match (&item, card.as_ref().and_then(|b| b.base_defences).filter(|d| d.iter().any(|v| *v > 0.0))) {
            (Some(it), Some(w)) => {
                let have = ["Armour", "Evasion Rating", "Energy Shield"].map(|k| it.props.get(k).copied().unwrap_or(0.0));
                Some(if w.iter().zip(have).any(|(w, h)| *w > 0.0 && h <= 0.0) {
                    missing
                } else if w.iter().zip(have).all(|(w, h)| h >= *w) {
                    good
                } else {
                    partial
                })
            }
            _ => None,
        };
        // Implicites visés présents sur la pièce (même ligne aux valeurs près ; une compétence,
        // la même).
        let worn_implicits: Vec<String> = worn.as_ref().map(|w| w.implicits.clone()).unwrap_or_default();
        let build_impl_marks: Vec<Option<(&str, Color32)>> = build_impl
            .iter()
            .map(|l| {
                worn.as_ref()?;
                if let Some(skill) = l.strip_prefix(tr!("Octroie l'Aptitude : ")) {
                    let same = worn_implicits.iter().any(|w| w.strip_prefix(tr!("Octroie l'Aptitude : ")).is_some_and(|n| n.contains(skill)));
                    return Some(if same { good } else { missing });
                }
                let (tpl, want) = trade::template(l);
                Some(match worn_implicits.iter().map(|w| trade::template(w)).find(|(t, _)| *t == tpl) {
                    Some((_, have)) if have.unwrap_or(0.0) < want.unwrap_or(0.0) => partial,
                    Some(_) => good,
                    None => missing,
                })
            })
            .collect();
        // Runes conseillées posées sur la pièce (chacune ne compte qu'une fois).
        let mut build_runes: Vec<ui::RuneLine> = match &worn_side {
            Some(ws) => ws.runes.clone(),
            None => card.as_ref().map(|b| b.runes.clone()).unwrap_or_default(),
        };
        if let (Some(w), None) = (&worn, &worn_side) {
            let mut have: Vec<String> = w.runes.iter().map(|r| r.name.clone()).collect();
            for r in &mut build_runes {
                r.mark = Some(match have.iter().position(|h| *h == r.name) {
                    Some(i) => {
                        have.remove(i);
                        good
                    }
                    None => missing,
                });
            }
        }
        // Mods visés : ✓ atteint, ~ en dessous, ✗ absent (sans pièce : tels quels).
        let target_line = |have: &compare::Have| -> Option<(&'static str, Color32)> {
            match have {
                compare::Have::Full(_) => Some(good),
                compare::Have::Partial(_) => Some(partial),
                compare::Have::Missing => Some(missing),
                compare::Have::Unknown => None,
            }
        };
        // Largeur naturelle de chaque colonne : sa ligne la plus longue (en-tête, mods, runes, marché).
        let (left_w, right_w) = {
            let text_w = |t: &str, size: f32| ui.fonts(|f| f.layout_no_wrap(t.to_string(), egui::FontId::proportional(size), Color32::WHITE).size().x);
            let (body, small) = (14.0, ui::HEADER_LINE_SIZE);
            let lines_w = |ls: &[String], size: f32| ls.iter().map(|l| text_w(l, size)).fold(0.0_f32, f32::max);
            let joined = |p: &ui::Parts| p.iter().map(|(t, _)| t.as_str()).collect::<String>();
            let head_w = |title: &str, after: Option<&str>, def: &ui::Parts, props: &[ui::Parts], imp: &[String]| {
                let t = text_w(&gametext::name(title), 17.0) + after.map_or(0.0, |a| 8.0 + text_w(&gametext::name(a), 13.5));
                let props_w = props.iter().map(|p| text_w(&joined(p), small)).fold(0.0_f32, f32::max);
                let imp: Vec<String> = imp.iter().map(|l| gametext::implicit(l).into_owned()).collect();
                90.0 + t.max(text_w(&joined(def), small)).max(props_w).max(lines_w(&imp, small))
            };
            let rune_w = |rs: &[ui::RuneLine]| rs.iter().map(|r| 26.0 + text_w(&format!("{}{}", gametext::name(&r.name), r.rest), body)).fold(0.0_f32, f32::max);
            let left = match &worn {
                Some(w) => ui::worn_width(ui, w).max(text_w(tr!("CORRESPONDANCE AU BUILD"), small)),
                None => head_w(tr!("Pas suivie"), Some(tr!("Ctrl+Alt+C sur ta pièce en jeu")), &vec![], &[], &[]),
            };
            let right = match (&worn_side, &card) {
                (Some(ws), _) => {
                    let lines: Vec<String> = ws.lines.iter().map(|(_, _, l)| format!("✗  {}", gametext::line(l))).chain(ws.notes.iter().cloned()).collect();
                    head_w(&ws.name, ws.after.as_deref(), &vec![], &ws.props, &ws.implicits).max(lines_w(&lines, body)).max(rune_w(&ws.runes)) + 14.0
                }
                (None, Some(b)) => {
                    let title = unique.as_deref().unwrap_or(&b.base);
                    let mut targets: Vec<String> = match &unique_text {
                        Some(t) => t.mods.iter().map(|m| format!("✗  {}", gametext::line(m))).collect(),
                        None => b.lines.iter().map(|(l, _)| format!("✗  {}", gametext::line(l))).collect(),
                    };
                    targets.extend(build_buffs.iter().cloned());
                    let market = [price.as_ref().map(|(t, _)| text_w(t, 16.0)).unwrap_or(0.0), basis.as_deref().map_or(0.0, |t| text_w(t, small)), link.as_ref().map_or(0.0, |(_, t)| text_w(t, body) + 20.0)];
                    head_w(title, unique.as_ref().and(unique_base.as_deref()), &build_def, &build_flask, &build_impl)
                        .max(lines_w(&targets, body))
                        .max(rune_w(&b.runes))
                        .max(market.into_iter().fold(0.0_f32, f32::max))
                }
                (None, None) => text_w(tr!("Rien de recommandé"), 17.0) + 90.0,
            };
            // Le titre (et sa croix) doit tenir sur la largeur des deux colonnes.
            let min_total = text_w(&title, 17.0) + 40.0;
            let (l, r) = (left.max(200.0) + 8.0, right.max(200.0) + 8.0);
            let extra = (min_total - (l + r + 24.0)).max(0.0) / 2.0;
            (l + extra, r + extra)
        };
        // Agrandissement : ce que permet la place, marges gardées (6 % de chaque côté, au moins
        // 24 px), par pas de 0,05 pour ne pas osciller ; la hauteur gardée est celle à l'échelle 1.
        let margin = (zone.width() * 0.06).max(MODAL_MARGIN);
        let fit = ((zone.width() - 2.0 * margin - 68.0) / (left_w + right_w)).min((zone.height() - 2.0 * margin) / natural);
        let k = ((fit * 20.0).floor() / 20.0).clamp(k_min, k_max);
        let (left_w, right_w) = (left_w * k, right_w * k);
        let natural = natural * k;
        for font in ui.style_mut().text_styles.values_mut() {
            font.size *= k;
        }
        ui.spacing_mut().item_spacing *= k;
        // Colonnes + écart + marges + barre de défilement, réduites ensemble si la place manque.
        let wanted = left_w + right_w + 24.0 + 32.0 + 12.0;
        let width = wanted.min(zone.width() - 2.0 * MODAL_MARGIN);
        let shrink = ((width - 24.0 - 32.0 - 12.0) / (left_w + right_w)).min(1.0);
        let (left_w, right_w) = (left_w * shrink, right_w * shrink);
        let panel = egui::Rect::from_center_size(zone.center(), Vec2::new(width, natural.min(zone.height() - 2.0 * MODAL_MARGIN)));
        // Le panneau garde ses clics (sinon le fond les prendrait, ou les cases dessous).
        ui.interact(panel, ui.id().with("slot_detail_panel"), egui::Sense::click());
        let mod_color = Color32::from_rgb(136, 136, 255);
        let dim = Color32::GRAY;
        let mut close = false;
        let mut content_h = 0.0;
        let frame = egui::Frame::new().fill(Color32::from_rgb(12, 11, 10)).stroke(egui::Stroke::new(1.0_f32, ui::GOLD_DIM)).corner_radius(6);
        let shown = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(panel), |ui| {
            frame
                .inner_margin(16)
                .show(ui, |ui| {
                    ui.set_width(width - 32.0);
                    ui::gold_widgets(ui);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&title).color(ui::GOLD).size(17.0 * k).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| close = ui::text_button(ui, "✕").clicked());
                    });
                    // Trait doré sous le titre, comme sous les titres de section (Historique, Mapping…).
                    ui.add_space(4.0);
                    let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
                    ui.painter().hline(line.x_range(), line.center().y, egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.6)));
                    // En-tête (titre, trait) au-dessus de la partie qui défile.
                    let top = ui.cursor().top() - (panel.top() + 16.0);
                    let out = egui::ScrollArea::vertical().max_height(panel.bottom() - 16.0 - ui.cursor().top()).auto_shrink([false, true]).show(ui, |ui| {
                        egui::Grid::new("slot_detail_grid").num_columns(2).spacing([24.0, 5.0]).show(ui, |ui| {
                            // Titres de colonne, qui fixent aussi la largeur de chaque colonne.
                            let widths = [left_w, right_w];
                            ui.allocate_ui_with_layout(Vec2::new(left_w, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_min_width(left_w);
                                ui.label(RichText::new(left_title).color(ui::GOLD_DIM).small().strong());
                            });
                            ui.allocate_ui_with_layout(Vec2::new(right_w, 0.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_min_width(right_w);
                                ui.label(RichText::new(right_title).color(ui::GOLD_DIM).small().strong());
                            });
                            ui.end_row();
                            // En tête de chaque colonne : l'image (case de même taille des deux côtés), et à
                            // sa droite, alignés sur son haut, le nom, les défenses et les implicites.
                            // En-tête commun à toute l'app : défenses et implicites en petit sous le titre.
                            // Les deux en-têtes prennent la hauteur du plus haut, calés en haut (la grille
                            // centrerait le plus court).
                            let head_h_id = ui.id().with("detail_head_h");
                            let head_h: f32 = ui.data(|d| d.get_temp(head_h_id)).unwrap_or(0.0);
                            let mut tallest: f32 = 0.0;
                            let mut cell = |ui: &mut egui::Ui, col_w: f32, add: &mut dyn FnMut(&mut egui::Ui) -> egui::Rect| {
                                ui.allocate_ui_with_layout(Vec2::new(col_w, head_h), egui::Layout::top_down(egui::Align::Min), |ui| {
                                    ui.set_min_height(head_h);
                                    // Un peu d'air sous le titre de la colonne.
                                    ui.add_space(6.0);
                                    tallest = tallest.max(add(ui).height() + 6.0);
                                });
                            };
                            cell(ui, left_w, &mut |ui| match &worn {
                                Some(w) => ui::worn_header_scaled(ui, "detail_worn", w, k),
                                None => ui::item_header_scaled(ui, "detail_worn", None, tr!("Pas suivie"), dim, Some(tr!("Ctrl+Alt+C sur ta pièce en jeu")), vec![], None, k, false),
                            });
                            // Un rare n'a pas de nom fixe : son type de base en tient lieu.
                            let build_tex = card.as_ref().and_then(|b| b.image.as_ref());
                            let mut build_props: Vec<ui::Parts> = Vec::new();
                            let mut build_prop_marks = Vec::new();
                            if !build_def.is_empty() {
                                build_props.push(build_def.clone());
                                build_prop_marks.push(build_def_mark);
                            }
                            build_props.extend(build_flask.iter().cloned());
                            let build_lines = ui::header_lines_scaled(&build_props, &build_prop_marks, &build_impl, &build_impl_marks, k);
                            cell(ui, right_w, &mut |ui| match (&worn_side, &unique, &card) {
                                (Some(ws), _, _) => {
                                    let lines = ui::header_lines_full(&ws.weapon, &ws.props, &ws.prop_marks, &ws.implicits, &ws.implicit_marks, k);
                                    ui::item_header_scaled(ui, "detail_build", ws.image.as_ref(), &ws.name, ws.name_color, ws.after.as_deref(), lines, None, k, false)
                                }
                                (None, Some(u), _) => ui::item_header_scaled(ui, "detail_build", build_tex, u, ui::rarity_color(item::Rarity::Unique), unique_base.as_deref(), build_lines.clone(), None, k, false),
                                (None, None, Some(b)) if !b.base.is_empty() => {
                                    ui::item_header_scaled(ui, "detail_build", build_tex, &b.base, ui::rarity_color(rec_rarity), None, build_lines.clone(), None, k, false)
                                }
                                _ => ui::item_header_scaled(ui, "detail_build", build_tex, tr!("Rien de recommandé"), dim, None, vec![], None, k, false),
                            });
                            if (tallest - head_h).abs() > 0.5 {
                                ui.data_mut(|d| d.insert_temp(head_h_id, tallest));
                                ui.ctx().request_repaint();
                            }
                            ui.end_row();
                            for w in widths {
                                ui.add_sized([w, 6.0], egui::Separator::default().horizontal());
                            }
                            ui.end_row();
                            // Mods : ceux de la pièce comparés au build (✓ ~ • ✗), ceux que vise le build.
                            ui.vertical(|ui| {
                                if let Some(w) = &worn {
                                    ui::worn_mods(ui, w);
                                }
                            });
                            ui.vertical(|ui| {
                                // Face à la pièce portée : les écarts si on la remplace.
                                if let Some(ws) = &worn_side {
                                    let font = egui::TextStyle::Body.resolve(ui.style());
                                    for (mark, color, text) in &ws.lines {
                                        let mut job = egui::text::LayoutJob::default();
                                        if let Some((m, c)) = mark {
                                            job.append(&format!("{m}  "), 0.0, egui::TextFormat::simple(font.clone(), *c));
                                        }
                                        job.append(&gametext::line(text), 0.0, egui::TextFormat::simple(font.clone(), *color));
                                        ui.label(job);
                                    }
                                    for n in &ws.notes {
                                        ui.label(RichText::new(n).color(dim).italics());
                                    }
                                    return;
                                }
                                // Effet du charme, puis les mods : ceux de l'unique tels quels (plages
                                // de valeurs du jeu), sinon ceux que vise le build.
                                for l in &build_buffs {
                                    ui.label(RichText::new(gametext::line(l)).color(mod_color));
                                }
                                match (&unique_text, &card) {
                                    // Unique : ses mods (plages du jeu), comparés à la pièce portée.
                                    (Some(t), _) => {
                                        let parts = item.as_ref().map(|it| unique_matches(it, &t.mods));
                                        let font = egui::TextStyle::Body.resolve(ui.style());
                                        for (i, m) in t.mods.iter().enumerate() {
                                            let mut job = egui::text::LayoutJob::default();
                                            match parts.as_ref().and_then(|p| p.get(i)) {
                                                Some(&part) => {
                                                    let (mark, color) = match_mark(part);
                                                    job.append(&format!("{mark}  "), 0.0, egui::TextFormat::simple(font.clone(), color));
                                                    job.append(&gametext::line(m), 0.0, egui::TextFormat::simple(font.clone(), color));
                                                }
                                                None => job.append(&gametext::line(m), 0.0, egui::TextFormat::simple(font.clone(), mod_color)),
                                            }
                                            ui.label(job);
                                        }
                                    }
                                    (None, Some(b)) => {
                                        let font = egui::TextStyle::Body.resolve(ui.style());
                                        for (l, have) in &b.lines {
                                            let mut job = egui::text::LayoutJob::default();
                                            match target_line(have) {
                                                Some((mark, color)) => {
                                                    job.append(&format!("{mark}  "), 0.0, egui::TextFormat::simple(font.clone(), color));
                                                    job.append(&gametext::line(l), 0.0, egui::TextFormat::simple(font.clone(), color));
                                                }
                                                None => job.append(&gametext::line(l), 0.0, egui::TextFormat::simple(font.clone(), mod_color)),
                                            }
                                            ui.label(job);
                                        }
                                    }
                                    _ => {}
                                }
                            });
                            ui.end_row();
                            // Runes posées, runes conseillées.
                            let worn_has = worn.as_ref().is_some_and(|w| !w.runes.is_empty() || w.empty_sockets > 0);
                            if worn_has || !build_runes.is_empty() {
                                ui.vertical(|ui| {
                                    if let Some(w) = &worn {
                                        ui::worn_runes(ui, w, left_w);
                                    }
                                });
                                ui.vertical(|ui| {
                                    if !build_runes.is_empty() {
                                        ui::rune_divider(ui, right_w);
                                    }
                                    ui::rune_lines(ui, &build_runes);
                                });
                                ui.end_row();
                            }
                            // Bas des colonnes (score, marché) : son trait seulement s'il a un contenu,
                            // la rangée entière seulement si l'une des deux en a un.
                            let left_score = worn.as_ref().and_then(|w| w.score);
                            let right_has = match &worn_side {
                                Some(ws) => ws.score.is_some(),
                                None => price.is_some() || basis.is_some() || link.is_some(),
                            };
                            if left_score.is_none() && !right_has {
                                return;
                            }
                            for (w, has) in widths.into_iter().zip([left_score.is_some(), right_has]) {
                                if has {
                                    ui.add_sized([w, 6.0], egui::Separator::default().horizontal());
                                } else {
                                    ui.allocate_exact_size(Vec2::new(w, 6.0), egui::Sense::hover());
                                }
                            }
                            ui.end_row();
                            // Correspondance de la pièce au build, en face du prix de la recommandation.
                            ui.vertical(|ui| {
                                if let Some(score) = worn.as_ref().and_then(|w| w.score) {
                                    ui::score_block(ui, score);
                                }
                            });
                            ui.vertical(|ui| {
                                // Face à la pièce portée : son score, en face de celui de la pièce de gauche.
                                if let Some(ws) = &worn_side {
                                    if let Some(score) = ws.score {
                                        ui::score_block(ui, score);
                                    }
                                    return;
                                }
                                if !right_has {
                                    return;
                                }
                                ui.label(RichText::new(tr!("AU MARCHÉ")).color(ui::GOLD_DIM).small().strong());
                                if let Some((text, color)) = &price {
                                    ui.horizontal(|ui| {
                                        if pending {
                                            ui.add(egui::Spinner::new().color(ui::GOLD));
                                        }
                                        ui.label(RichText::new(text).color(*color).size(16.0 * k).strong());
                                    });
                                }
                                if let Some(b) = &basis {
                                    ui.label(RichText::new(b).color(dim).small());
                                }
                                if let Some((url, text)) = &link {
                                    ui.add_space(2.0);
                                    ui.hyperlink_to(*text, url);
                                }
                            });
                            ui.end_row();
                        });
                    });
                    // Un peu de marge : sans elle, un arrondi fait apparaître la barre de défilement.
                    content_h = top + out.content_size.y + 34.0;
                });
        });
        let _ = shown;
        if (content_h - natural).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(height_id, content_h / k));
            ui.ctx().request_repaint();
        }
        if close || backdrop.clicked() {
            self.slot_detail = None;
        }
    }

    /// Niveau du personnage suivi (100 sans personnage : rien n'est jugé trop haut).
    fn char_level(&self) -> u32 {
        self.coach.as_ref().and_then(|c| c.character().map(|(_, ch)| ch.level)).unwrap_or(100)
    }

    /// Colonne « actuellement équipé » d'une pièce : en-tête (image, titre, propriétés, implicites),
    /// mods comparés à ce que vise le build, runes. La même dans le détail et au survol de la case.
    fn worn_view(&mut self, slot: Slot, it: &Item, card: Option<&ui::BuildCard>) -> ui::WornView {
        let keys = ["Armour", "Evasion Rating", "Energy Shield"];
        let mut props = Vec::new();
        let def = defence_parts(keys.map(|k| it.props.get(k).copied().unwrap_or(0.0)), keys.map(|k| it.props.contains_key(&format!("{k}_aug"))));
        if !def.is_empty() {
            props.push(def);
        }
        // Arme d'attaque : ses dégâts par seconde, avec la base (ils suivent les lignes d'arme quand
        // elles sont là).
        if let Some(d) = compare::weapon_dps(it).filter(|_| weapon_lines(it).is_empty()) {
            props.push(dps_parts(&d));
        }
        let p = |k: &str| (it.props.get(k).copied().unwrap_or(0.0), it.props.contains_key(&format!("{k}_aug")));
        props.extend(flask_parts(p("recover"), p("recover_mana").0 > 0.0, p("recover_time"), p("duration"), p("charges_use"), p("charges_max")));
        if it.req_level > 0 {
            props.push(ui::req_level_parts(it.req_level, self.char_level()));
        }
        // Implicites et compétences données par la pièce.
        let implicits: Vec<String> = it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k == item::ModKind::Implicit).map(|(m, _)| m.clone())
            .chain(it.granted_skills.iter().map(|s| format!("{}{s}", tr!("Octroie l'Aptitude : "))))
            .collect();
        // Mods de la pièce telle qu'elle est (la comparaison est côté recommandation) : en blanc
        // ceux que le build vise, en gris les autres.
        let targets: Vec<String> = card.map(|b| b.lines.iter().map(|(l, _)| trade::template(l).0).collect()).unwrap_or_default();
        let mods = it
            .mods
            .iter()
            .zip(&it.mod_kinds)
            .filter(|(_, k)| matches!(k, item::ModKind::Explicit | item::ModKind::Enchant))
            .map(|(m, _)| {
                let wanted = targets.is_empty() || targets.contains(&trade::template(m).0);
                ("", if wanted { Color32::from_gray(225) } else { Color32::from_gray(120) }, m.clone())
            })
            .collect();
        // Châsses vides : à remplir ; si le build n'y conseille rien, les augments qui collent à ses
        // objectifs.
        let empty_sockets = self.socketed(it).map_or(0, |s| s.empty());
        let tips = if empty_sockets > 0 && self.recommended_runes(slot).is_empty() { self.socket_tips(it, slot) } else { vec![] };
        let suggestions = tips.into_iter().map(|(name, rest)| self.rune_line(name, rest, Some(&it.item_class))).collect();
        // Runes posées, sans prix : on ne peut pas les retirer de la pièce.
        let mut runes = self.socketed(it).map(|s| self.rune_list(&s.augments, Some(&it.item_class))).unwrap_or_default();
        for r in &mut runes {
            r.rest.clear();
        }
        // Correspondance au build : moyenne de ce qu'il vise ici, chaque élément à sa part de la
        // valeur visée (absent 0) : mods, implicites de la base (une compétence : la même ou
        // non), runes conseillées (posée ou non). Ce qui dépasse la valeur visée ne compte qu'une
        // fois tout atteint (le score passe alors 100 %, chaque élément plafonné à 200 %). Un
        // unique visé : ses mods, au moins le minimum de chaque plage.
        let ratio = |have: f64, want: Option<f64>| want.filter(|w| *w > 0.0).map_or(1.0, |w| (have / w).clamp(0.0, 2.0));
        let score = match card {
            // Unique visé : ses mods face à ceux de la pièce (au moins le minimum de chaque plage).
            Some(b) if b.unique.is_some() => {
                let mods = b.unique.as_ref().and_then(|u| self.coach.as_ref()?.catalog().uniques.get(u).map(|t| t.mods.clone())).unwrap_or_default();
                let parts = unique_matches(it, &mods);
                if parts.is_empty() {
                    Some(if b.unique.as_deref() == Some(it.name.as_str()) { 100 } else { 0 })
                } else {
                    let capped = parts.iter().map(|p| p.min(1.0)).sum::<f64>() / parts.len() as f64;
                    let full = parts.iter().sum::<f64>() / parts.len() as f64;
                    Some((100.0 * if capped >= 0.999 { full } else { capped }).round() as u32)
                }
            }
            Some(b) => {
                let mut parts: Vec<f64> = b
                    .lines
                    .iter()
                    .map(|(l, have)| {
                        let want = trade::template(l).1;
                        match have {
                            compare::Have::Full(v) => ratio(*v, want).max(1.0),
                            compare::Have::Partial(v) => ratio(*v, want).min(1.0),
                            _ => 0.0,
                        }
                    })
                    .collect();
                for want in &b.implicits {
                    parts.push(if let Some(skill) = want.strip_prefix(tr!("Octroie l'Aptitude : ")) {
                        if implicits.iter().any(|h| h.strip_prefix(tr!("Octroie l'Aptitude : ")).is_some_and(|n| n.contains(skill))) { 1.0 } else { 0.0 }
                    } else {
                        let (tpl, w) = trade::template(want);
                        implicits.iter().map(|h| trade::template(h)).find(|(t, _)| *t == tpl).map_or(0.0, |(_, h)| ratio(h.unwrap_or(0.0), w))
                    });
                }
                let mut posed: Vec<&str> = runes.iter().map(|r| r.name.as_str()).collect();
                for r in &b.runes {
                    parts.push(match posed.iter().position(|p| *p == r.name) {
                        Some(i) => {
                            posed.remove(i);
                            1.0
                        }
                        None => 0.0,
                    });
                }
                (!parts.is_empty()).then(|| {
                    let mean = |v: &mut dyn Iterator<Item = f64>| v.sum::<f64>() / parts.len() as f64;
                    let capped = mean(&mut parts.iter().map(|p| p.min(1.0)));
                    let score = if capped >= 0.999 { mean(&mut parts.iter().copied()) } else { capped };
                    (100.0 * score).round() as u32
                })
            }
            None => None,
        };
        let (title, after) = ui::item_title(it);
        ui::WornView {
            weapon: weapon_lines(it),
            score,
            empty_sockets,
            suggestions,
            image: self.assets.texture(it),
            title: title.to_string(),
            after: after.map(String::from),
            color: ui::rarity_color(it.rarity),
            props,
            implicits,
            mods,
            runes,
        }
    }

    /// Fiche d'un unique attendu, au format de celle d'une pièce portée : image, nom et base,
    /// défenses et niveau requis de la base, implicites et mods (texte PoB, plages du jeu).
    fn unique_view(&mut self, name: &str) -> ui::WornView {
        let text = self.coach.as_ref().and_then(|c| c.catalog().uniques.get(name).cloned()).unwrap_or_default();
        let catalog = self.augments.get();
        let mut props = Vec::new();
        if let Some(d) = catalog.as_ref().and_then(|c| c.defences.get(&text.base).copied()) {
            let def = defence_parts(d, [false; 3]);
            if !def.is_empty() {
                props.push(def);
            }
        }
        if let Some(level) = catalog.as_ref().and_then(|c| c.req_levels.get(&text.base).copied()) {
            props.push(ui::req_level_parts(level, self.char_level()));
        }
        ui::WornView {
            weapon: vec![],
            image: self.assets.texture_unique(name),
            title: name.to_string(),
            after: Some(text.base.clone()).filter(|b| !b.is_empty()),
            color: ui::rarity_color(item::Rarity::Unique),
            props,
            implicits: text.implicits.clone(),
            // Effet du charme pendant son utilisation, puis les mods.
            mods: self
                .coach
                .as_ref()
                .and_then(|c| c.catalog().charm_buffs.get(&text.base).cloned())
                .unwrap_or_default()
                .into_iter()
                .chain(text.mods.iter().cloned())
                .map(|m| ("", Color32::from_gray(225), m))
                .collect(),
            runes: vec![],
            empty_sockets: 0,
            score: None,
            suggestions: vec![],
        }
    }

    /// Fiche d'un type de base, au format de celle d'une pièce portée : image, défenses, niveau
    /// requis et implicites de la base.
    fn base_view(&mut self, name: &str) -> ui::WornView {
        let catalog = self.augments.get();
        let mut props = Vec::new();
        if let Some(d) = catalog.as_ref().and_then(|c| c.defences.get(name).copied()) {
            let def = defence_parts(d, [false; 3]);
            if !def.is_empty() {
                props.push(def);
            }
        }
        if let Some(level) = catalog.as_ref().and_then(|c| c.req_levels.get(name).copied()) {
            props.push(ui::req_level_parts(level, self.char_level()));
        }
        ui::WornView {
            weapon: vec![],
            image: self.assets.texture_by_name(name),
            title: name.to_string(),
            after: None,
            color: Color32::from_gray(225),
            props,
            implicits: catalog.as_ref().and_then(|c| c.implicits.get(name).cloned()).unwrap_or_default(),
            mods: vec![],
            runes: vec![],
            empty_sockets: 0,
            score: None,
            suggestions: vec![],
        }
    }

    /// Unique que le build recommande sur `slot` à cette étape (une fiole de mana est rangée par
    /// le planificateur avec les fioles de vie : son type de base la remet à sa place).
    fn recommended_unique(&self, step: &guide::Step, slot: Slot) -> Option<String> {
        let prices = self.prices.data.lock().unwrap();
        let base = |n: &str| prices.as_ref()?.unique_by_name(n).map(|(b, _, _)| b.to_string());
        // Les charmes sont tous rangés en Charm1 : leur place dans la rangée dit la case.
        let lands = |w: &guide::Wanted| match uniques::slot(w.slot, base(&w.name).as_deref()) {
            Some(Slot::Charm1) => gear::CHARMS.get(w.position).copied() == Some(slot),
            s => s == Some(slot),
        };
        step.uniques.iter().find(|w| lands(w)).map(|w| w.name.clone())
    }

    /// Pièce recommandée sur une case vide, en noir et blanc : l'unique, sinon le type de base.
    fn recommended_ghost(&mut self, slot: Slot) -> Option<egui::TextureHandle> {
        let (_, step) = self.basis()?;
        if let Some(name) = self.recommended_unique(&step, slot) {
            return self.assets.texture_unique_gray(&name);
        }
        let base = Some(step.base_for(slot).to_string()).filter(|b| !b.is_empty())?;
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
        let tips = s.augments.iter().map(|n| self.rune_line(n.clone(), String::new(), Some(&it.item_class))).collect();
        Some(ui::SocketView { filled, empty: s.empty(), unknown: s.unknown, ghosts, tips })
    }

    fn compare(&self, it: &Item) -> Option<compare::Comparison> {
        self.compare_at(it, None)
    }

    /// `compare`, face à un emplacement choisi pour un anneau ou un charme.
    fn compare_at(&self, it: &Item, at: Option<Slot>) -> Option<compare::Comparison> {
        let basis = self.basis();
        let catalog = self.augments.get();
        let base = self.assets.index.lock().unwrap().as_ref().and_then(|i| i.base_of(it)).unwrap_or_else(|| it.base.clone());
        let item_base = catalog.as_ref().and_then(|c| c.base(&base));
        compare::compare_at(it, &self.gear, basis.as_ref().map(|(n, s)| (n.as_str(), s)), item_base, &|n| catalog.as_ref()?.base(n), at)
    }

    /// Emplacements où peut aller un anneau (les deux), un charme (ceux ouverts) ou une arme (ses
    /// deux sets) ; vide pour le reste.
    fn multi_slots(&self, it: &Item) -> Vec<Slot> {
        match gear::kind_of(&it.item_class) {
            Some(gear::Kind::TwoHanded | gear::Kind::Single(Slot::Weapon)) => vec![Slot::Weapon, Slot::Weapon2],
            Some(gear::Kind::Single(Slot::Offhand)) => vec![Slot::Offhand, Slot::Offhand2],
            Some(gear::Kind::Ring) => gear::RINGS.to_vec(),
            Some(gear::Kind::Charm) => gear::CHARMS[..self.gear.charm_slots().unwrap_or(gear::CHARMS.len()).max(1)].to_vec(),
            _ => Vec::new(),
        }
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
            self.say(tr!("Build actif : {name}", name));
        }
    }

    fn import_files(&mut self, files: Vec<builds::BuildFile>, source: &str) {
        match self.builds.add(files, source) {
            Ok(meta) => {
                let id = meta.id.clone();
                self.activate_build(&id);
                self.add_build.message = Some((true, tr!("« {name} » ajouté et activé", name = meta.name)));
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
                (None, _) => RichText::new(tr!("personnage ?")).color(Color32::GRAY),
            };
            let resp = egui::ComboBox::from_id_salt("character_select").width(width(0)).truncate().selected_text(label).show_ui(ui, |ui| {
                for (name, class, level) in &chars {
                    if ui.selectable_label(current_char.as_ref() == Some(name), tr!("{name} — {class} niv. {level}", name, class = gametext::name(class), level)).clicked() {
                        picked = Some(name.clone());
                    }
                }
            });
            if !confirmed && has_character {
                resp.response.on_hover_text(tr!("Personnage supposé d'après le journal du jeu : à confirmer"));
            }
            });
        }
        if let Some(name) = picked {
            if let Some(c) = &mut self.coach {
                c.set_character(&name);
            }
            self.switch_character(Some(name));
        }

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
                .selected_text(RichText::new(&self.league).color(ui::GOLD))
                .show_ui(ui, |ui| {
                    for l in list {
                        if ui.selectable_label(l == self.league, &l).clicked() {
                            league_pick = Some(l);
                        }
                    }
                })
                .response
                .on_hover_text(tr!("Ligue de ce personnage, pour les prix du marché (le journal du jeu ne la donne pas)"));
            });
        }

        let current = self.builds.active().map(|b| b.name.clone());
        let selected = match &current {
            Some(n) => RichText::new(n).color(ui::GOLD),
            None => RichText::new(tr!("aucun build")).color(Color32::GRAY),
        };
        let mut chosen: Option<String> = None;
        // Cadre de la liste des builds : le bouton de lien prend exactement sa hauteur.
        let mut combo_rect: Option<egui::Rect> = None;
        in_zone(ui, 1, &mut |ui| {
            let combo = egui::ComboBox::from_id_salt("build_select").width(width(1)).truncate().selected_text(selected.clone()).show_ui(ui, |ui| {
            for b in self.builds.list() {
                let active = self.builds.active().is_some_and(|a| a.id == b.id);
                if ui.selectable_label(active, &b.name).on_hover_text(tr!("source : {source}", source = b.source)).clicked() {
                    chosen = Some(b.id.clone());
                }
            }
        })
        .response
        .on_hover_text(current.as_deref().unwrap_or(tr!("aucun build")));
            combo_rect = Some(combo.rect);
        });

        let mut step_pick: Option<String> = None;
        if let Some(shown_label) = &shown_step {
            in_zone(ui, 3, &mut |ui| {
            let preview = self.step_preview.is_some();
            let mut text = RichText::new(if preview { tr!("{step} (aperçu)", step = guide::step_text(shown_label)) } else { guide::step_text(shown_label) }).color(ui::GOLD);
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
                        let name = if current { tr!("{step} (actuelle)", step = guide::step_text(label)) } else { guide::step_text(label) };
                        if ui.selectable_label(Some(label) == shown_step.as_ref(), name).clicked() {
                            step_pick = Some(label.clone());
                        }
                    }
                })
                .response
                .on_hover_text(if preview {
                    tr!("Étape vue en aperçu : reviens sur l'étape « (actuelle) » pour suivre ta progression")
                } else {
                    tr!("Étape du build : en endgame, choisis la variante que tu joues ; les autres s'affichent en aperçu")
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
                Some(step) => tr!("Ouvrir le build sur son site, à l'étape {step}", step = guide::step_text(step)),
                None => tr!("Ouvrir le build sur son site").to_string(),
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
                    self.say(tr!("Impossible d'ouvrir le navigateur : {e}", e));
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
                let button = egui::Button::new(RichText::new(tr!("⬆  Version {version} disponible — mettre à jour", version)).color(Color32::from_rgb(120, 200, 120)));
                if ui.add(button).on_hover_text(tr!("Tu as la version {version}", version = updater::VERSION)).clicked() {
                    self.updater.install(ui.ctx());
                }
            }
            State::Downloading { version } => {
                ui.label(RichText::new(tr!("téléchargement de la version {version}…", version)).color(Color32::GRAY));
                ui.spinner();
            }
            State::Ready { version } => {
                if ui.button(RichText::new(tr!("Version {version} installée — redémarrer", version)).color(Color32::from_rgb(120, 200, 120))).clicked() {
                    match updater::restart() {
                        Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.say(tr!("Redémarrage impossible : {e} (relance l'app à la main)", e)),
                    }
                }
            }
            State::Failed(e) => {
                ui.label(RichText::new(tr!("mise à jour impossible")).color(Color32::from_rgb(230, 120, 100))).on_hover_text(e);
            }
        }
    }

    /// Copie dans le presse-papier tout l'équipement suivi, avec son contexte (personnage,
    /// ligue, build), en Markdown.
    fn export_gear(&mut self, ctx: &egui::Context) {
        let count = self.gear.slots.len();
        let character = self.coach.as_ref().and_then(|c| c.character().map(|(n, s)| (n.to_string(), s.class.clone(), s.level)));
        let title = match &character {
            Some((name, ..)) => tr!("Équipement de {name} (Path of Exile 2)", name),
            None => tr!("Équipement (Path of Exile 2)").to_string(),
        };
        let mut context = Vec::new();
        if let Some((_, class, level)) = &character {
            context.push(tr!("Classe : {class}, niveau {level}", class, level));
        }
        if !self.league.is_empty() {
            context.push(tr!("Ligue : {league}", league = self.league));
        }
        if let Some(b) = self.builds.active() {
            let step = self.judging_step().map(|s| tr!(" (étape : {step})", step = guide::step_text(&s.label)));
            context.push(tr!("Build suivi : {name}{step}", name = b.name, step = step.unwrap_or_default()));
        }
        context.push(tr!("Texte des pièces tel que copié en jeu (Ctrl+Alt+C), en anglais").into());
        ctx.copy_text(self.gear.export(&title, &context));
        self.say(trn!(count, "Équipement copié dans le presse-papier ({n} pièce)", "Équipement copié dans le presse-papier ({n} pièces)"));
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
        let (_, closed) = ui::modal(ctx, "settings", tr!("Réglages"), true, 620.0, |ui| {
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
                    section(ui, tr!("Interface"));
                    label(ui, tr!("Langue"), tr!("textes de l'app et du jeu ; Alt maintenu : l'autre langue"));
                    egui::ComboBox::from_id_salt("lang")
                        .width(ui.available_width())
                        .selected_text(RichText::new(draft.lang.name()).color(ui::GOLD))
                        .show_ui(ui, |ui| {
                            for l in i18n::Lang::ALL {
                                ui.selectable_value(&mut draft.lang, l, l.name());
                            }
                        });
                    ui.end_row();

                    section(ui, tr!("Jeu"));
                    let check = paths::check(&draft.game_dir);
                    let found = match (check.log, check.assets) {
                        (true, true) => tr!("journal et images du jeu trouvés"),
                        (true, false) => tr!("journal trouvé, images du jeu introuvables"),
                        (false, true) => tr!("images trouvées, journal du jeu introuvable"),
                        (false, false) => tr!("jeu introuvable dans ce dossier"),
                    };
                    label(ui, tr!("Dossier du jeu"), found);
                    // Le chemin est le bouton : un clic ouvre le choix du dossier.
                    let dir = draft.game_dir.display().to_string();
                    let shown = if dir.is_empty() {
                        tr!("choisir le dossier…").to_string()
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
                            tr!("Choisir le dossier du jeu").to_string()
                        } else {
                            tr!("{dir}\nclic : choisir un autre dossier", dir)
                        });
                    if resp.clicked() {
                        let (tx, rx) = channel();
                        let ctx = ctx.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(
                                rfd::FileDialog::new()
                                    .set_title(tr!("Dossier de Path of Exile 2"))
                                    .pick_folder(),
                            );
                            ctx.request_repaint();
                        });
                        draft.picking = Some(rx);
                    }
                    ui.end_row();

                    section(ui, tr!("Diagnostic (Ctrl+C)"));
                    label(ui, tr!("Affichage"), tr!("ce que fait Ctrl+C sur un objet en jeu"));
                    egui::ComboBox::from_id_salt("open_on_copy")
                        .width(ui.available_width())
                        .selected_text(
                            RichText::new(if draft.open_on_copy {
                                tr!("ouvrir le détail du diagnostic")
                            } else {
                                tr!("rester sur l'historique")
                            })
                            .color(ui::GOLD),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut draft.open_on_copy,
                                true,
                                tr!("ouvrir le détail du diagnostic"),
                            );
                            ui.selectable_value(
                                &mut draft.open_on_copy,
                                false,
                                tr!("rester sur l'historique"),
                            );
                        });
                    ui.end_row();

                    section(ui, tr!("Prix"));
                    label(
                        ui,
                        tr!("Estimation sur le trade"),
                        tr!("recherche d'annonces comparables sur pathofexile.com"),
                    );
                    ui.checkbox(&mut draft.trade_prices, tr!("activée"));
                    ui.end_row();
                    label(
                        ui,
                        tr!("Lancement de l'estimation"),
                        tr!("au Ctrl+C, ou plus tard avec le bouton « Estimer »"),
                    );
                    ui.add_enabled_ui(draft.trade_prices, |ui| {
                        egui::ComboBox::from_id_salt("auto_estimate")
                            .width(ui.available_width())
                            .selected_text(
                                RichText::new(if draft.auto_estimate {
                                    tr!("automatique, dès le Ctrl+C")
                                } else {
                                    tr!("manuel, bouton « Estimer »")
                                })
                                .color(ui::GOLD),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut draft.auto_estimate,
                                    true,
                                    tr!("automatique, dès le Ctrl+C"),
                                );
                                ui.selectable_value(
                                    &mut draft.auto_estimate,
                                    false,
                                    tr!("manuel, bouton « Estimer »"),
                                );
                            });
                    });
                    ui.end_row();
                    label(
                        ui,
                        tr!("Seuil du marché"),
                        tr!("en dessous, un objet est conseillé au PNJ"),
                    );
                    // Le nombre, puis sa devise dans une petite liste à la taille de son contenu.
                    ui.horizontal(|ui| {
                        ui::amount_with_unit(ui, "market_min_unit", &mut draft.market_min_ex, &mut draft.market_min_unit, &["ex", "chaos", "div"], 0.1..=10_000.0);
                    });
                    ui.end_row();

                    section(ui, tr!("Mises à jour"));
                    label(
                        ui,
                        tr!("Recherche automatique"),
                        tr!("sur GitHub, au lancement puis toutes les 5 minutes"),
                    );
                    ui.checkbox(&mut draft.auto_update, tr!("activée"));
                    ui.end_row();

                    section(ui, tr!("Expérimental"));
                    label(
                        ui,
                        tr!("Onglet Boss"),
                        tr!("boss d'endgame affrontés : passages, morts, temps (expérimental)"),
                    );
                    ui.checkbox(&mut draft.boss_tab, tr!("afficher l'onglet"));
                    ui.end_row();
                });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui::action_button(ui, tr!("✔  Enregistrer")).clicked();
                    cancel = ui::action_button(ui, tr!("Annuler")).clicked();
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
            self.stored.market_min_unit = draft.market_min_unit.clone();
            self.config.market_min_unit = draft.market_min_unit.clone();
            self.market_min_unit = draft.market_min_unit.clone();
            self.stored.boss_tab = draft.boss_tab;
            self.config.trade_prices = draft.trade_prices;
            self.config.auto_update = draft.auto_update;
            self.config.market_min_ex = draft.market_min_ex;
            self.updater.set_enabled(draft.auto_update);
            if draft.lang != i18n::base() {
                self.stored.lang = draft.lang.code().to_string();
                self.config.lang = self.stored.lang.clone();
                i18n::set(draft.lang);
                // Textes mis en cache dans l'ancienne langue.
                self.socket_tips.clear();
            }
            if draft.game_dir != self.config.game_dir {
                self.set_game_dir(ctx, draft.game_dir);
            }
            self.stored.save(&self.dirs);
            self.say(tr!("Réglages enregistrés").into());
        } else if cancel || closed {
            self.settings = None;
        }
    }

    fn changelog_window(&mut self, ctx: &egui::Context) {
        let Some(releases) = &self.changelog else { return };
        let (ok, closed) = ui::modal(ctx, "changelog", tr!("Nouveautés"), true, 560.0, |ui| {
            ui::plain_widgets(ui);
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                for (i, r) in releases.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(10.0);
                    }
                    let current = r.version == updater::VERSION;
                    let title = if current { tr!("Version {version} (installée)", version = r.version) } else { tr!("Version {version}", version = r.version) };
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
        let button = ui.add_enabled(!regex.is_empty(), egui::Button::new(RichText::new(tr!("🔍  Regex marchands")).color(ui::GOLD)));
        let button = button
            .on_hover_ui(|ui| {
                ui.set_max_width(420.0);
                let step = step.as_ref().map(|s| guide::step_text(&s.label)).unwrap_or_default();
                ui.label(RichText::new(tr!("Copie une recherche pour les marchands, d'après le build ({step}) : colle-la (Ctrl+V) dans la recherche d'un marchand en jeu, les objets utiles s'allument.", step)).size(15.0));
                ui.add_space(4.0);
                ui.label(RichText::new(tr!("Cherche : {names}", names = names.join(", "))).size(15.0).color(Color32::GRAY));
            })
            .on_disabled_hover_text(tr!("Aucun build suivi, ou il ne vise aucun mod à cette étape"));
        if button.clicked() {
            ui.ctx().copy_text(regex.clone());
            self.say(tr!("Regex copiée ({n} caractères) : colle-la dans la recherche d'un marchand", n = regex.chars().count()));
        }
    }

    /// Bouton « Menu » de la barre d'état, ouvert vers le haut : actions, puis réglages.
    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        ui::gold_widgets(ui);
        let button = ui.button(RichText::new(tr!("⚙  Menu")).color(ui::GOLD));
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
            ui::menu_section(ui, tr!("BUILD ET JEU"));
            if ui::menu_item(ui, "⚔", tr!("Gestionnaire de builds"), tr!("ajouter, mettre à jour, choisir le build suivi"), true, false).clicked() {
                self.add_build.open = true;
                self.add_build.message = None;
            }
            if ui::menu_item(ui,
                    "🔧",
                    tr!("Réglages"),
                    tr!("langue, dossier du jeu, prix, mises à jour"),
                    true,
                    false,
                )
                .clicked()
                {
                    self.settings = Some(SettingsDraft::new(&self.stored, &self.config));
            }
            ui.add_space(4.0);
            ui.separator();
            ui::menu_section(ui, tr!("DONNÉES"));
            let tracked = !self.gear.slots.is_empty();
            let export_hint = if tracked { tr!("copie le texte de chaque pièce suivie") } else { tr!("aucune pièce suivie : Ctrl+Alt+C sur une pièce") };
            if ui::menu_item(ui, "📤", tr!("Exporter l'équipement"), export_hint, tracked, false).clicked() {
                self.export_gear(ui.ctx());
            }
            if ui::menu_item(ui, "⟳", tr!("Mettre à jour les catalogues"), tr!("augments, types de base, gemmes, images"), true, false).clicked() {
                self.augments.refresh();
                self.gem_infos.refresh();
                self.assets.refresh();
                self.say(tr!("Mise à jour des catalogues en cours (quelques secondes)").into());
            }
            if ui::menu_item(ui, "✨", tr!("Nouveautés"), &tr!("ce qui a changé jusqu'à la version {version}", version = updater::VERSION), true, false).clicked() {
                self.changelog = Some(changelog::releases());
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);
            if ui::menu_item(ui, "🗑", tr!("Vider l'historique"), "", !self.history.is_empty(), true).clicked() {
                self.history.clear();
                self.save_history();
                self.say(tr!("Historique vidé").into());
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
                        self.build_rarities = None;
                        self.build_jewels = None;
                        self.build_guide = None;
                        // Build en cours d'utilisation : ses nouvelles étapes tout de suite.
                        if self.builds.active().is_some_and(|b| b.id == id) {
                            let files = self.builds.files(&id);
                            if let Some(c) = &mut self.coach {
                                c.reload_steps(&files);
                            }
                        }
                        (true, tr!("« {name} » mis à jour", name = meta.name))
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
        let (_, closed) = ui::modal(ctx, "build_manager", tr!("Gestionnaire de builds"), true, 680.0, |ui| {
                // Liste des builds : une carte par build, actions à droite.
                if self.builds.list().is_empty() {
                    ui.label(RichText::new(tr!("Aucun build pour l'instant.")).color(Color32::GRAY));
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
                                    if ui.add_enabled(i > 0, egui::Button::new(RichText::new("▲").small()).small()).on_hover_text(tr!("Monter")).clicked() {
                                        shift = Some((b.id.clone(), -1));
                                    }
                                    if ui.add_enabled(i + 1 < count, egui::Button::new(RichText::new("▼").small()).small()).on_hover_text(tr!("Descendre")).clicked() {
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
                                            ui.label(RichText::new(tr!("● actif")).color(Color32::from_rgb(110, 210, 110)).small());
                                        }
                                    });
                                    if web {
                                        ui.hyperlink_to(RichText::new(short_source(&b.source)).small(), &b.source).on_hover_text(&b.source);
                                    } else {
                                        ui.label(RichText::new(tr!("{source} · sans lien", source = b.source)).color(Color32::GRAY).small());
                                    }
                                    let busy = self.add_build.link_pending.as_ref().is_some_and(|(id, _, _)| *id == b.id);
                                    if busy {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label(RichText::new(tr!("mise à jour…")).color(Color32::GRAY).small());
                                        });
                                    } else if self.add_build.link_for.as_ref() == Some(&b.id) {
                                        ui.horizontal(|ui| {
                                            ui.add(egui::TextEdit::singleline(&mut self.add_build.link_url).hint_text(tr!("lien du build sur Mobalytics ou Kami-labs")).desired_width(text_w - 60.0));
                                            if ui.add_enabled(!self.add_build.link_url.trim().is_empty(), egui::Button::new(tr!("Lier"))).clicked() {
                                                link = Some((b.id.clone(), self.add_build.link_url.trim().to_string()));
                                            }
                                        });
                                    } else if !web && ui::text_button(ui, tr!("lier sa page web (Mobalytics, Kami-labs)")).clicked() {
                                        self.add_build.link_for = Some(b.id.clone());
                                        self.add_build.link_url.clear();
                                    }
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if self.add_build.confirm_remove.as_ref() == Some(&b.id) {
                                        if ui.button(tr!("Annuler")).clicked() {
                                            self.add_build.confirm_remove = None;
                                        }
                                        if ui.button(RichText::new(tr!("Confirmer")).color(Color32::from_rgb(230, 100, 90))).clicked() {
                                            remove = Some(b.id.clone());
                                        }
                                    } else {
                                        if ui.button(RichText::new(tr!("Supprimer")).color(Color32::from_rgb(220, 120, 110))).clicked() {
                                            self.add_build.confirm_remove = Some(b.id.clone());
                                        }
                                        // De droite à gauche : Activer, Mettre à jour, Supprimer à l'écran.
                                        if web
                                            && ui
                                                .add_enabled(self.add_build.link_pending.is_none(), egui::Button::new(tr!("Mettre à jour")))
                                                .on_hover_text(tr!("Retélécharge le build depuis sa page (étapes, objets, runes)"))
                                                .clicked()
                                        {
                                            link = Some((b.id.clone(), b.source.clone()));
                                        }
                                        if !active && ui.button(tr!("Activer")).clicked() {
                                            activate = Some(b.id.clone());
                                        }
                                    }
                                });
                            });
                        });
                }

                ui.add_space(12.0);
                ui::section_header(ui, tr!("AJOUTER UN BUILD"), None, |_| {});
                // Sites pris en charge, en couleur.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(tr!("Lien de la page du build, sur "));
                    ui.label(RichText::new("Mobalytics").color(Color32::from_rgb(90, 200, 190)).strong());
                    ui.label(tr!(" ou "));
                    ui.label(RichText::new("Kami-labs").color(Color32::from_rgb(240, 160, 70)).strong());
                    ui.label(tr!(" :"));
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.add_build.url).hint_text("https://mobalytics.gg/poe-2/…").desired_width(ui.available_width() - 90.0));
                    let busy = self.add_build.pending.is_some();
                    if ui.add_enabled(!busy && !self.add_build.url.trim().is_empty(), egui::Button::new(tr!("Importer"))).clicked() {
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
            let dir = paths::character_dir(&self.data_dir, n);
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
        // Une copie en jeu affiche son résultat dans les panneaux : on quitte le plein écran.
        self.full_view = false;
        // Waystone : noté pour la prochaine map (onglet Maps), pas diagnostiqué.
        if let Some(w) = mapping::Waystone::parse(&ev.text) {
            let title = tr!("Pierre de téléportation T{tier} « {name} »", tier = w.tier, name = w.name);
            match self.coach.as_mut().map(|c| c.set_waystone(w)) {
                Some(true) => {
                    self.side_tab = views::SideTab::Maps;
                    self.say(tr!("{title} : associé à la prochaine map ouverte", title));
                }
                _ => self.say(tr!("{title} : personnage pas encore détecté, impossible de l'associer à une map", title)),
            }
            return;
        }
        let Some(it) = item::parse(&ev.text) else { return };
        match ev.kind {
            CopyKind::Diagnose => {
                self.side_tab = views::SideTab::Diagnostic;
                self.say(tr!("Diagnostic : {item}", item = it.title()));
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
                    Equipped::Placed { slot, replaced } => tr!(
                        "{item} suivi en {slot}{replaced}",
                        item = it.title(),
                        slot = slot.label().to_lowercase(),
                        replaced = replaced.map(|r| tr!(" (remplace {r})", r)).unwrap_or_default()
                    ),
                    Equipped::AlreadyThere(slot) => {
                        tr!("{item} est déjà suivi ({slot})", item = it.title(), slot = slot.label().to_lowercase())
                    }
                    Equipped::CharmsFull => {
                        tr!("3 charmes déjà suivis et aucun {base} : retire-en un (clic droit)", base = it.base)
                    }
                    Equipped::UnknownClass(c) => tr!("Je ne sais pas où se porte un objet « {c} »", c),
                };
                match self.gear.save() {
                    Ok(()) => self.say(msg),
                    Err(e) => self.say(tr!("Impossible d'enregistrer l'équipement : {e}", e)),
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
        let threshold = self.threshold_ex();
        let mut value = {
            let data = self.prices.data.lock().unwrap();
            rules::evaluate(it, data.as_ref(), threshold)
        };
        if let Some(est @ trade::Estimate::Done { total, .. }) = self.trade.get(it) {
            if let Some(estimate) = est.value_ex().filter(|_| total >= 3) {
                if estimate >= threshold {
                    value.label = rules::Label::Market;
                    value.summary = tr!("{total} annonces comparables, estimé ≈ {price}", total, price = self.fmt_price(estimate));
                } else {
                    value.label = rules::Label::Vendor;
                    value.summary = self.below_threshold(estimate);
                }
            } else if total == 0 {
                value.label = rules::Label::Vendor;
                value.summary = tr!("aucune annonce comparable en achat immédiat : sans valeur au marché").into();
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

    /// Seuil du marché en exalts, aux taux du moment (tel quel tant que les cours manquent).
    fn threshold_ex(&self) -> f64 {
        let data = self.prices.data.lock().unwrap();
        let rate = match (self.market_min_unit.as_str(), data.as_ref()) {
            ("div", Some(p)) if p.exalted_per_divine > 0.0 => p.exalted_per_divine,
            ("chaos", Some(p)) => p.currency("Chaos Orb").map(|c| p.to_ex(c)).unwrap_or(1.0),
            _ => 1.0,
        };
        self.market_min_ex * rate
    }

    /// Un objet classé « PNJ » malgré un prix : il vaut trop peu pour la peine de le vendre.
    fn below_threshold(&self, ex: f64) -> String {
        tr!(
            "vaut ≈ {price} au marché, sous le seuil de {threshold} : pas la peine de le mettre en vente, à vendre au PNJ",
            price = self.fmt_price(ex),
            threshold = Self::fmt_in(self.market_min_ex, (1.0, &self.market_min_unit))
        )
    }

    /// Annonces lues, en cartes : prix, âge, note, puis mods comparés à l'objet diagnostiqué
    /// (vert : l'annonce en a plus, rouge : moins, gris : mod que l'objet n'a pas ; en bas, ce
    /// qui manque à l'annonce). Les annonces proches d'abord ; les autres, en retrait, pèsent
    /// peu dans le prix.
    fn listing_cards(&mut self, ui: &mut egui::Ui, it: &Item, listings: &[trade::Listing], score: f64, shares: Option<Vec<f64>>) {
        let mine: Vec<(String, Option<f64>)> = it.mods.iter().map(|m| trade::template(m)).collect();
        let (green, red, gray) = (Color32::from_rgb(110, 200, 110), Color32::from_rgb(220, 95, 85), Color32::from_gray(140));
        let my_dps = weapon_total_dps(it);
        // Écart d'une valeur de l'annonce face à celle de l'objet : « +5,5 », « −1 », et sa couleur.
        let diff = |d: f64| {
            let n = if (d - d.round()).abs() < 0.05 { format!("{}", d.abs().round()) } else { i18n::decimal(format!("{:.1}", d.abs())) };
            (format!("{}{n}", if d > 0.0 { "+" } else { "−" }), if d > 0.0 { green } else { red })
        };
        // Chaque annonce en objet : ses lignes d'arme (DPS compris), défenses et implicites.
        let as_items: Vec<Option<Item>> = listings.iter().map(|l| l.as_item(&it.item_class)).collect();
        let note = |v: f64| i18n::decimal(format!("{v:.1}"));
        ui.add_space(4.0);
        // Les proches d'abord, puis les autres ; chaque groupe du moins cher au plus cher, avec leur
        // part dans le prix et leur image (chargée à la demande).
        let ordered: Vec<(&trade::Listing, Option<f64>, Option<Item>)> = listings
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kept)
            .chain(listings.iter().enumerate().filter(|(_, l)| !l.kept))
            .map(|(i, l)| (l, shares.as_ref().and_then(|s| s.get(i).copied()), as_items[i].clone()))
            .collect();
        let images: Vec<Option<egui::TextureHandle>> = ordered.iter().map(|(l, _, _)| (!l.icon.is_empty()).then(|| self.assets.texture_url(l.icon.clone())).flatten()).collect();
        // Autant de colonnes que la largeur tient de cartes d'au moins 280 px ; les cartes prennent
        // toute la largeur (une colonne : toute la colonne).
        const CARD_MIN: f32 = 280.0;
        const PAD: f32 = 16.0 + 2.0;
        let gap = 8.0;
        let width = ui.available_width();
        let columns = (((width + gap) / (CARD_MIN + PAD + gap)).floor() as usize).max(1);
        let card_w = ((width - gap * (columns - 1) as f32) / columns as f32 - PAD).max(120.0);
        let card = |ui: &mut egui::Ui, l: &trade::Listing, share: Option<f64>, image: Option<&egui::TextureHandle>, li: Option<&Item>| {
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
            // L'âge seul (« 3 h ») pour l'infobulle, et la phrase (« il y a 3 h ») pour la carte.
            let since = match l.age_hours {
                Some(h) if h < 1.0 => Some(tr!("moins d'une heure").to_string()),
                Some(h) if h < 48.0 => Some(tr!("{n} h", n = h.round())),
                Some(h) => Some(tr!("{n} j", n = (h / 24.0).round())),
                None => None,
            };
            let age = match (&since, l.age_hours) {
                (_, Some(h)) if h < 1.0 => tr!("à l'instant").to_string(),
                (Some(s), _) => tr!("il y a {age}", age = s),
                _ => String::new(),
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
                                    ui.label(RichText::new(gametext::name(&l.name)).color(c(ui::rarity_color(rarity))).size(15.0).strong());
                                }
                                ui.label(RichText::new(gametext::name(&l.base)).color(c(ui::rarity_color(rarity))).size(14.0));
                                ui.label(RichText::new(tr!("niv. {level}", level = l.item_level)).color(c(Color32::GRAY)).size(13.0));
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
                        ui.label(tr!("Niveau de l'objet {level}", level = l.item_level));
                        if !defences.is_empty() {
                            ui.label(defences.join(" · "));
                        }
                        ui.label(tr!("Prix demandé : {amount} {currency}", amount, currency = l.currency));
                        if let Some(ex) = l.ex {
                            ui.label(self.price_all(ex));
                        }
                        if let Some(s) = &since {
                            ui.label(tr!("En vente depuis : {age}", age = s));
                        }
                    });
                    // Lignes d'arme (DPS et écart face à l'objet), défenses, implicites (écart) : comme
                    // l'en-tête d'une pièce.
                    let mut lines: Vec<Vec<(String, Color32)>> = li.map(weapon_lines).unwrap_or_default();
                    if let (Some(mine_dps), Some(theirs)) = (my_dps, li.and_then(weapon_total_dps)) {
                        if let Some(line) = lines.iter_mut().find(|l| l.first().is_some_and(|(t, _)| t.starts_with(tr!("Attaques par seconde")))) {
                            if (theirs - mine_dps).abs() >= 0.5 {
                                let (t, col) = diff(theirs - mine_dps);
                                line.push((format!("  {t}"), col));
                            }
                        }
                    }
                    let def: Vec<String> = [tr!("Armure"), tr!("Score d'Évasion"), tr!("Bouclier d'énergie")].iter().zip(l.defences).filter(|(_, v)| *v > 0.0).map(|(n, v)| format!("{n} {v:.0}")).collect();
                    if !def.is_empty() {
                        lines.push(vec![(def.join(" · "), Color32::GRAY)]);
                    }
                    let mut seen = Vec::new();
                    for m in &l.implicits {
                        let (t, v) = trade::template(m);
                        let mut line = vec![(gametext::line(m).into_owned(), ui::MOD_COLOR)];
                        if let Some((_, mv)) = mine.iter().find(|(mt, _)| *mt == t) {
                            seen.push(t.clone());
                            if let (Some(a), Some(b)) = (v, mv) {
                                if (a - b).abs() >= 0.05 {
                                    let (d, col) = diff(a - b);
                                    line.push((format!("  ({d})"), col));
                                }
                            }
                        }
                        lines.push(line);
                    }
                    if !lines.is_empty() {
                        ui.add_space(4.0);
                        for line in ui::header_lines_full(&lines.iter().map(|l| l.iter().map(|(t, col)| (t.clone(), c(*col))).collect()).collect::<Vec<_>>(), &[], &[], &[], &[], 1.0) {
                            ui.label(line);
                        }
                    }
                    // Note et part dans le prix, sur une ligne, entre deux traits discrets.
                    let rule = |ui: &mut egui::Ui| {
                        ui.add_space(3.0);
                        let r = ui.available_rect_before_wrap();
                        ui.painter().hline(r.x_range(), r.top(), egui::Stroke::new(1.0_f32, c(Color32::from_gray(48))));
                        ui.add_space(4.0);
                    };
                    if score > 0.0 {
                        // La note en blanc, sa part dans le prix en gris.
                        let text = if l.kept { tr!("note {score}", score = note(l.score)) } else { tr!("éloignée : note {score} ({ratio}×)", score = note(l.score), ratio = note(l.score / score)) };
                        rule(ui);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(text).color(c(Color32::from_gray(225))).size(13.0));
                            if let Some(sh) = share {
                                let pct = sh * 100.0;
                                let part = if pct < 1.0 { tr!("· pèse moins de 1 % dans le prix").to_string() } else { tr!("· pèse {pct} % dans le prix", pct = pct.round()) };
                                ui.label(RichText::new(part).color(c(Color32::GRAY)).size(13.0));
                            }
                        });
                        rule(ui);
                    } else {
                        ui.add_space(4.0);
                    }
                    for m in l.mods.iter().filter(|m| !l.implicits.contains(m)) {
                        let (t, v) = trade::template(m);
                        let shown = gametext::line(m);
                        let mut text = shown.to_string();
                        let color = match mine.iter().find(|(mt, _)| *mt == t) {
                            Some((_, mv)) => {
                                seen.push(t.clone());
                                match (v, mv) {
                                    (Some(a), Some(b)) if (a - b).abs() >= 0.05 => {
                                        let (d, col) = diff(a - b);
                                        text = format!("{shown}  ({d})");
                                        col
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
                            ui.label(RichText::new(tr!("sans : {line}", line = gametext::line(line))).color(c(red.gamma_multiply(0.8))).size(14.0).italics());
                        }
                    }
                });
            });
        };
        egui::Grid::new("listings_grid").num_columns(columns).spacing([gap, gap]).show(ui, |ui| {
            for (i, ((l, share, li), image)) in ordered.iter().zip(&images).enumerate() {
                card(ui, l, *share, image.as_ref(), li.as_ref());
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
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(left.shrink2(Vec2::new(12.0, 6.0))), |ui| self.left_panel(ui));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(right.shrink2(Vec2::new(12.0, 6.0))), |ui| {
            let summary = self.coach.as_ref().map(|c| c.summary()).unwrap_or_else(|| tr!("chargement du guide…").into());
            ui::section_header_sub(ui, tr!("ACTIVITÉ"), &summary);
            self.feed_list(ui);
        });
    }

    /// Bloc en bas à gauche : la route de leveling, puis les maps une fois l'endgame commencé.
    fn left_panel(&mut self, ui: &mut egui::Ui) {
        let has_maps = self.coach.as_mut().and_then(|c| c.maps()).is_some_and(|b| !b.runs.is_empty());
        if has_maps && self.bottom_maps.unwrap_or(true) {
            self.mapping_panel(ui);
        } else {
            self.route_panel(ui, has_maps);
        }
    }

    /// Maps de l'endgame : la map en cours et son chronomètre, la séance (l'historique est dans
    /// l'onglet Maps).
    fn mapping_panel(&mut self, ui: &mut egui::Ui) {
        let mut to_route = false;
        ui::section_header(ui, tr!("MAPPING"), None, |ui| {
            to_route = ui::header_button(ui, tr!("Route")).on_hover_text(tr!("Revoir la route de leveling et ce qui a été oublié pendant la campagne")).clicked();
        });
        if to_route {
            self.bottom_maps = Some(false);
        }
        let Some(c) = &mut self.coach else { return };
        let now = c.log_now();
        let Some(book) = c.maps() else { return };
        let dim = Color32::GRAY;
        // Une map quittée depuis longtemps n'est plus en cours (son instance a expiré).
        let recent = |t: i64, max: i64| now.is_none_or(|n| n - t < max);
        match book.current().filter(|(r, inside)| inside.is_some() || recent(r.last, 15 * 60)) {
            Some((run, inside)) => {
                let secs = now.and_then(|n| book.live_secs(n)).unwrap_or(run.active_secs());
                ui.horizontal(|ui| {
                    tier_badge(ui, &mut self.passive_icons, run, 44.0, dim);
                    ui.label(RichText::new(run_name(run)).color(ui::GOLD).size(20.0).strong());
                    ui.label(group_label(run));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if inside.is_some() { Color32::from_gray(230) } else { dim };
                        ui.label(RichText::new(clock_text(secs)).color(color).size(28.0).monospace()).on_hover_text(time_note(run));
                    });
                });
                ui.horizontal(|ui| {
                    match inside {
                        Some(_) => ui.label(RichText::new(tr!("en cours")).color(Color32::from_rgb(110, 200, 110)).size(13.5)),
                        // Dehors après y être entré : retour en ville, le chrono est arrêté.
                        None => ui.label(RichText::new(tr!("en pause")).color(Color32::from_rgb(230, 160, 70)).size(13.5)),
                    };
                    ui.add_space(10.0);
                    icon_count(ui, &mut self.passive_icons, TOMB_ICON, run.deaths, &trn!(run.deaths, "{n} mort", "{n} morts"), Color32::from_rgb(225, 90, 80));
                    ui.add_space(10.0);
                    icon_count(ui, &mut self.passive_icons, PORTAL_ICON, run.returns(), &trn!(run.returns(), "{n} retour en ville", "{n} retours en ville"), Color32::from_rgb(110, 150, 255));
                });
                if inside.is_some() {
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                }
            }
            None => {
                ui.label(RichText::new(tr!("Pas de map en cours : le chronomètre démarre à l'entrée dans une map.")).color(dim).size(15.0));
                if let Some(w) = book.pending_waystone() {
                    ui.label(RichText::new(tr!("Pierre de téléportation T{tier} « {name} » prête pour la prochaine map ({mods} mods)", tier = w.tier, name = w.name, mods = w.mods.len())).color(Color32::LIGHT_GRAY).size(13.5));
                }
            }
        }
        // Séance : les maps enchaînées sans longue pause, en tableau. La map en cours y compte
        // avec son temps en direct.
        let open = book.current().map(|(r, _)| r as *const mapping::Run);
        let live = now.and_then(|n| book.live_secs(n));
        let secs = |r: &mapping::Run| if Some(r as *const _) == open { live.unwrap_or(r.active_secs()) } else { r.active_secs() };
        let sitting = book.sitting();
        let maps: Vec<&mapping::Run> = sitting.iter().filter(|r| r.kind == mapping::RunKind::Map).collect();
        if !sitting.is_empty() {
            let bosses = sitting.len() - maps.len();
            let total: i64 = sitting.iter().map(secs).sum();
            let map_secs: i64 = maps.iter().map(|r| secs(r)).sum();
            let deaths: u32 = sitting.iter().map(|r| r.deaths).sum();
            let returns: u32 = sitting.iter().map(|r| r.returns()).sum();
            ui.add_space(12.0);
            ui.add(egui::Separator::default().spacing(0.0));
            ui.add_space(12.0);
            let title = if sitting.last().is_some_and(|r| recent(r.last, 2 * 3600)) { tr!("SÉANCE") } else { tr!("DERNIÈRE SÉANCE") };
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(ui::GOLD_DIM).size(13.0).strong());
                ui.label(RichText::new(tr!("depuis le {date}", date = date_text(sitting[0].start))).color(dim).size(13.5));
            });
            ui.add_space(6.0);
            let mut rows: Vec<(&str, String)> = Vec::new();
            rows.push((tr!("Maps"), trn!(maps.len(), "{n} map", "{n} maps")));
            if bosses > 0 {
                rows.push((tr!("Boss"), trn!(bosses, "{n} arène", "{n} arènes")));
            }
            rows.push((tr!("Temps de jeu"), duration_text(total)));
            if !maps.is_empty() {
                rows.push((tr!("Temps moyen par map"), duration_text(map_secs / maps.len() as i64)));
                if map_secs > 0 {
                    rows.push((tr!("Maps par heure"), i18n::decimal(format!("{:.1}", maps.len() as f64 * 3600.0 / map_secs as f64))));
                }
            }
            let per = |n: u32| if sitting.is_empty() { String::new() } else { tr!(" · {n} par map", n = i18n::decimal(format!("{:.2}", n as f64 / sitting.len() as f64))) };
            rows.push((tr!("Morts"), format!("{deaths}{}", per(deaths))));
            rows.push((tr!("Retours en ville"), format!("{returns}{}", per(returns))));
            // Deux moitiés quand la largeur le permet : chacune son tableau libellé / valeur, le
            // même écart entre libellé et valeur des deux côtés.
            let pairs = if ui.available_width() >= 520.0 { 2 } else { 1 };
            let per_half = rows.len().div_ceil(pairs);
            ui.columns(pairs, |cols| {
                for (h, col) in cols.iter_mut().enumerate() {
                    egui::Grid::new(("sitting", h)).num_columns(2).spacing([20.0, 7.0]).show(col, |ui| {
                        for (label, value) in rows.iter().skip(h * per_half).take(per_half) {
                            ui.label(RichText::new(*label).color(dim).size(13.5));
                            ui.label(RichText::new(value).color(Color32::from_gray(215)).size(15.0));
                            ui.end_row();
                        }
                    });
                }
            });
        }
    }

    /// Petite modale de choix d'une récompense modifiable de la route : les options, celle du
    /// build marquée ★ ; un clic l'enregistre et ferme.
    fn route_pick_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.route_pick.clone() else { return };
        let quest_choices = self.quest_choices();
        let Some(c) = &mut self.coach else { return };
        let Some(r) = c.route.iter().find(|r| r.id == id).cloned() else { return };
        let current = c.character().and_then(|(_, ch)| ch.picks.get(&id)).and_then(|g| g.first()).cloned();
        let build: Vec<String> = route::build_choices(&r, &quest_choices).iter().map(|p| p.reward.clone()).collect();
        let mut pick = None;
        let (_, close) = ui::modal(ctx, "route_pick", r.what_text(), true, 360.0, |ui| {
            ui.label(RichText::new(tr!("Ton choix (modifiable en jeu en retournant sur place)")).color(Color32::GRAY).small());
            ui.add_space(4.0);
            for (short, full) in route::modifiable_options(&id) {
                let star = if build.iter().any(|b| b.eq_ignore_ascii_case(full)) { "★ " } else { "" };
                let selected = current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(full));
                let text = RichText::new(format!("{star}{full}")).color(if selected { Color32::from_rgb(136, 136, 255) } else { Color32::LIGHT_GRAY });
                if ui.selectable_label(selected, text).on_hover_text(i18n::tr(short)).clicked() {
                    pick = Some(full.to_string());
                }
            }
        });
        if let Some(text) = pick {
            c.set_pick(&id, &text);
            self.route_pick = None;
        } else if close {
            self.route_pick = None;
        }
    }

    /// Contenus facultatifs à ne pas rater (route de leveling), cochés tout seuls via le log.
    fn route_panel(&mut self, ui: &mut egui::Ui, has_maps: bool) {
        let quest_choices = self.quest_choices();
        let mut to_maps = false;
        let Some(c) = &mut self.coach else { return };
        // Ce que le joueur a pris pour les récompenses au choix (lu dans le journal).
        let player_picks = c.character().map(|(_, ch)| ch.picks.clone()).unwrap_or_default();
        let (act, items) = c.reminders();
        let label = match act {
            0 => tr!("ROUTE DE LEVELING").to_string(),
            5 => tr!("ROUTE — INTERLUDES").to_string(),
            a if a > 5 => tr!("ROUTE — CAMPAGNE TERMINÉE").to_string(),
            a => tr!("ROUTE — ACTE {act}", act = a),
        };
        ui::section_header(ui, &label, None, |ui| {
            if has_maps {
                to_maps = ui::header_button(ui, tr!("Mapping")).on_hover_text(tr!("Revenir aux maps : map en cours, séance, dernières maps")).clicked();
            }
        });
        let mut toggle: Option<String> = None;
        let mut open_pick: Option<String> = None;
        let out = egui::ScrollArea::vertical().id_salt("route").auto_shrink([false, false]).show(ui, |ui| {
            if items.is_empty() {
                let empty = match act {
                    0 => tr!("Elle s'affichera dès que le journal du jeu aura nommé ton personnage."),
                    a if a > 5 => tr!("Rien de la campagne n'a été oublié."),
                    _ => tr!("Rien à signaler pour cet acte."),
                };
                ui.label(RichText::new(empty).color(Color32::GRAY));
            }
            // Ce qui est fait, regroupé par acte sous un petit titre.
            let mut first = true;
            let mut done_act: Option<u32> = None;
            for (r, done) in items {
                if done && done_act != Some(r.act) {
                    if !first {
                        ui.add_space(8.0);
                    }
                    let title = if r.act == 5 { tr!("INTERLUDES").to_string() } else { tr!("ACTE {act}", act = r.act) };
                    ui.label(RichText::new(title).color(ui::GOLD_DIM).small().strong());
                    done_act = Some(r.act);
                }
                first = false;
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
                let mut reward = RichText::new(if pick.is_empty() { r.reward_text() } else { &pick }).color(color).strong();
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
                        ui.label(RichText::new(tr!("build ·")).color(ui::GOLD_DIM).small());
                    }
                    if behind && !done {
                        // L'« acte 5 » de la route, ce sont les interludes.
                        let when = if r.act == 5 { tr!("interludes").to_string() } else { tr!("acte {act}", act = r.act) };
                        ui.label(RichText::new(when).color(Color32::from_rgb(200, 150, 100)).small().italics());
                    }
                    // Récompense modifiable : libellé court au bout de la ligne (orange tant que le
                    // choix n'est pas connu), un clic ouvre le choix.
                    let options = route::modifiable_options(&r.id);
                    let with_pick = done && !options.is_empty();
                    let short = player_picks.get(&r.id).and_then(|g| g.first()).map(|p| options.iter().find(|(_, full)| full.eq_ignore_ascii_case(p)).map_or(p.as_str(), |(s, _)| i18n::tr(s)));
                    let tag = match short {
                        Some(s) => RichText::new(s).color(Color32::from_rgb(136, 136, 255)).small(),
                        None => RichText::new(tr!("à préciser")).color(ui::EMPTY_SOCKET).small(),
                    };
                    let tag_w = if with_pick { ui.fonts(|f| f.layout_no_wrap(tag.text().to_string(), egui::TextStyle::Small.resolve(ui.style()), Color32::WHITE).size().x) + 12.0 } else { 0.0 };
                    let text_w = ui.available_width() - tag_w;
                    let label = ui.allocate_ui(Vec2::new(text_w.max(40.0), ui.spacing().interact_size.y), |ui| ui.add(egui::Label::new(reward).truncate())).inner;
                    if with_pick {
                        let tag = ui.add(egui::Label::new(tag).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr!("Changer ce choix"));
                        if tag.clicked() {
                            open_pick = Some(r.id.clone());
                        }
                    }
                    (check, label)
                });
                // Fiche au survol, en blocs : la récompense, comment l'obtenir, où, le choix du build.
                let tip = |ui: &mut egui::Ui| {
                    ui.set_max_width(420.0);
                    ui.label(RichText::new(r.reward_text()).color(color).strong());
                    ui.label(RichText::new(r.what_text()).color(Color32::LIGHT_GRAY));
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr!("Zone : {zone}", zone = r.zone)).color(Color32::GRAY).small());
                    let importance = match r.importance.as_str() {
                        "must" => tr!("Indispensable"),
                        "recommended" => tr!("Conseillé"),
                        _ => tr!("Sur le chemin de l'histoire"),
                    };
                    ui.label(RichText::new(importance).color(Color32::GRAY).small());
                    if behind && !done {
                        let when = if r.act == 5 { tr!("Oubliée aux interludes").to_string() } else { tr!("Oubliée à l'acte {act}", act = r.act) };
                        ui.label(RichText::new(when).color(Color32::from_rgb(200, 150, 100)).small().italics());
                    }
                    // Ce que le joueur a pris (lu dans le journal du jeu).
                    if let Some(got) = player_picks.get(&r.id).filter(|g| !g.is_empty()) {
                        ui.separator();
                        ui.label(RichText::new(tr!("TON CHOIX")).color(ui::GOLD_DIM).small().strong());
                        for g in got {
                            ui.label(RichText::new(g).color(Color32::from_rgb(110, 210, 110)));
                        }
                    }
                    if !picks.is_empty() {
                        ui.separator();
                        ui.label(RichText::new(tr!("CHOIX DU BUILD")).color(ui::GOLD_DIM).small().strong());
                        for p in &picks {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&p.reward).color(ui::GOLD));
                                ui.label(RichText::new(format!("({})", p.quest)).color(Color32::GRAY).small());
                            });
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new(tr!("Cochée toute seule quand le jeu l'annonce ; clique la case pour corriger.")).color(Color32::DARK_GRAY).small().italics());
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
        if let Some(id) = open_pick {
            self.route_pick = Some(id);
        }
        if to_maps {
            self.bottom_maps = Some(true);
        }
    }

    fn feed_list(&mut self, ui: &mut egui::Ui) {
        let out = egui::ScrollArea::vertical().id_salt("feed").auto_shrink([false, false]).show(ui, |ui| {
            if self.feed.is_empty() {
                ui.label(RichText::new(tr!("Les niveaux, étapes du guide, morts et messages privés s'afficheront ici.")).color(Color32::GRAY));
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
        } else if self.tab == views::Tab::Atlas {
            self.atlas_view(ui, ui::place(area, scale, ui::VIEW_AREA));
        } else if self.tab == views::Tab::Guide {
            self.guide_view(ui, ui::place(area, scale, ui::VIEW_AREA));
        } else {
            self.build_view(ui, ui::place(area, scale, ui::VIEW_AREA));
        }
        if matches!(self.tab, views::Tab::Passives | views::Tab::Atlas) {
            self.full_view_button(ui, ui::place(area, scale, ui::VIEW_AREA));
        }
        // Détail d'une case : sur l'équipement ; ceux du diagnostic, sur n'importe quel onglet.
        // Détail d'une case : sur l'équipement. Ceux du diagnostic s'ouvrent sur toute la fenêtre
        // (voir `update`).
        if self.detail_mode == DetailMode::Worn {
            if equipment {
                self.slot_detail(ui, whole, 1.0, 1.8);
            } else {
                self.slot_detail = None;
            }
        }
        self.tab_row(ui, tabs);
    }

    /// Boutons du coin haut droit de la vue : recentrer, puis plein écran (entrée et sortie).
    fn full_view_button(&mut self, ui: &mut egui::Ui, view: egui::Rect) {
        let size = 30.0;
        let full = egui::Rect::from_min_size(egui::pos2(view.max.x - size, view.min.y), Vec2::splat(size));
        let center = full.translate(Vec2::new(-(size + 6.0), 0.0));
        let tip = if self.tab == views::Tab::Atlas { tr!("Recentrer (ou double-clic)") } else { tr!("Centrer sur l'ascendance (double-clic : tout le build)") };
        if ui::view_button(ui, center, ui::ViewIcon::Recenter).on_hover_text(tip).clicked() {
            self.recenter_view();
        }
        let (icon, tip) = if self.full_view { (ui::ViewIcon::Exit, tr!("Quitter le plein écran (Échap)")) } else { (ui::ViewIcon::Enter, tr!("Plein écran")) };
        if ui::view_button(ui, full, icon).on_hover_text(tip).clicked() {
            // La vue change de taille : elle se recadre d'elle-même.
            self.full_view = !self.full_view;
        }
    }

    fn recenter_view(&mut self) {
        if self.tab == views::Tab::Atlas {
            self.atlas_view.recenter();
        } else {
            self.tree.recenter();
        }
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
        let (steps, penalty, level) = match &self.coach {
            Some(c) => {
                let penalty = c
                    .character()
                    .map(|(_, ch)| stats::res_penalty(ch.max_act, ch.level))
                    .unwrap_or((0.0, tr!("acte 1").into()));
                (
                    &c.steps,
                    penalty,
                    c.character().map(|(_, ch)| ch.level),
                )
            }
            None => (&no_steps, (0.0, tr!("acte 1").into()), None),
        };
        let quests = self.quest_bonuses();
        let shown = self.shown_step_index().filter(|i| *i < steps.len());
        let gear = &mut self.gear;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal(|ui| {
                ui::gold_widgets(ui);
                let h = ui.min_rect().height().max(ui.spacing().interact_size.y);
                ui.label(RichText::new(tr!("Set d'armes")).color(ui::GOLD_DIM));
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
                        ui.label(RichText::new(tr!("Niveau")).color(ui::GOLD_DIM));
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
                .map(|s| tr!("Build · {step}", step = guide::step_text(&s.label)))
                .unwrap_or_else(|| tr!("Build (aucun)").into());
            views::stats(ui, &worn, build.as_ref(), &title, &penalty, quests.resistances);
        });
    }

    /// Onglet Atlas : l'arbre de l'Atlas du jeu, passifs posés allumés (journal du jeu).
    fn atlas_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let dim = Color32::GRAY;
        // L'arbre est lu dans les fichiers du jeu une fois, en arrière-plan (puis gardé en cache).
        if !self.atlas_loading {
            self.atlas_loading = true;
            let (slot, cache, game, ctx) = (self.atlas_tree.clone(), self.cache_dir.clone(), self.config.game_dir.clone(), ui.ctx().clone());
            std::thread::spawn(move || {
                let tree = atlas::load(&cache, &game);
                *slot.lock().unwrap() = Some(tree);
                ctx.request_repaint();
            });
        }
        let Some(book) = self.coach.as_mut().and_then(|c| c.maps()) else {
            ui.put(rect, egui::Label::new(RichText::new(tr!("Chargement du guide…")).color(dim)));
            return;
        };
        let allocated: HashSet<String> = book.atlas.keys().cloned().collect();
        match &*self.atlas_tree.lock().unwrap() {
            None => {
                ui.put(rect, egui::Label::new(RichText::new(tr!("Lecture de l'arbre de l'Atlas dans les fichiers du jeu…")).color(dim)));
            }
            Some(Err(e)) => {
                ui.put(rect, egui::Label::new(RichText::new(tr!("Arbre de l'Atlas indisponible : {e}", e)).color(dim)));
            }
            Some(Ok(tree)) => {
                let allocated: HashSet<&str> = allocated.iter().map(String::as_str).collect();
                self.atlas_view.show(ui, rect, tree, &allocated, &mut self.passive_icons);
            }
        }
    }

    /// Onglet Guide : les textes de l'auteur du build, sa présentation puis la description de la
    /// variante de l'étape choisie.
    fn guide_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        // La zone de l'onglet déborde un peu de la partie visible du cadre : le texte s'arrête au
        // bord visible.
        let rect = rect.intersect(ui.clip_rect());
        let dim = Color32::GRAY;
        let Some(coach) = self.coach.as_ref() else {
            ui.put(rect, egui::Label::new(RichText::new(tr!("Chargement du guide…")).color(dim)));
            return;
        };
        let Some(id) = self.builds.active().map(|b| b.id.clone()) else {
            ui.put(rect, egui::Label::new(RichText::new(tr!("Aucun build")).color(dim)));
            return;
        };
        if self.build_guide.as_ref().is_none_or(|(b, _)| *b != id) {
            self.build_guide = Some((id.clone(), builds::load_guide(&self.builds.dir_of(&id))));
        }
        let guide = self.build_guide.as_ref().and_then(|(_, g)| g.clone());
        let steps = coach.steps.clone();
        let shown = self.shown_step_index();
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            let Some(guide) = guide else {
                ui.label(RichText::new(tr!("Pas de texte de l'auteur pour ce build. Pour un build Mobalytics, mets-le à jour depuis le gestionnaire de builds pour le récupérer.")).color(dim));
                return;
            };
            if !i18n::english() {
                ui.label(RichText::new(tr!("Textes de l'auteur du build, en anglais.")).color(dim).small().italics());
                ui.add_space(4.0);
            }
            // Variante de l'étape affichée : son titre sur le site donne la même étiquette que
            // le fichier .build de l'étape.
            let label = shown.and_then(|i| steps.get(i)).map(|s| s.label.clone());
            let same = |title: &str| {
                let plain = title.split(" {").next().unwrap_or(title).trim();
                let from_title = guide::split_name(&format!("{title} - x")).0;
                label.as_deref().is_some_and(|l| from_title.as_deref() == Some(l) || plain == l)
            };
            let variant = guide.variants.iter().find(|(t, _)| same(t));
            // Un peu plus grand que le reste de l'app : c'est un texte à lire.
            const BODY: f32 = 15.5;
            let out = egui::ScrollArea::vertical().id_salt("build_guide").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(ui.available_width() - 12.0);
                if let Some(o) = &guide.overview {
                    ui::section_header(ui, tr!("PRÉSENTATION DU BUILD"), None, |_| {});
                    richtext::show(ui, "overview", &richtext::parse(o), BODY, self);
                    ui.add_space(14.0);
                }
                if let Some(l) = &label {
                    ui::section_header(ui, &tr!("VARIANTE · {step}", step = guide::step_text(l)), None, |_| {});
                    match variant {
                        Some((_, doc)) => richtext::show(ui, "variant", &richtext::parse(doc), BODY, self),
                        None => {
                            ui.label(RichText::new(tr!("L'auteur n'a rien écrit pour cette étape.")).color(dim));
                        }
                    }
                }
            });
            ui::fade_scroll(ui, &out, true);
        });
    }

    /// Onglets Sorts, Talents (ascendance au centre) et Uniques, pour l'étape choisie.
    fn build_view(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        // Joyaux de l'étape affichée, posés dans les châsses de l'arbre.
        let shown = self.shown_step_index().unwrap_or(0);
        let shown_label = self.coach.as_ref().and_then(|c| c.steps.get(shown).map(|s| s.label.clone()));
        let tree_jewels = self.tree_jewels(shown_label.as_deref().unwrap_or_default());
        let Some(coach) = self.coach.as_ref() else {
            ui.put(rect, egui::Label::new(RichText::new(tr!("Chargement du guide…")).color(Color32::GRAY)));
            return;
        };
        if coach.steps.is_empty() {
            // Sans build : explication et accès direct à l'ajout.
            let mut add = false;
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(rect.height() * 0.35);
                    ui::gold_widgets(ui);
                    ui.label(RichText::new(tr!("Aucun build")).color(ui::GOLD).heading());
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr!("Ajoute le build que tu suis pour voir ici ses aptitudes, son arbre de talents et son ascendance, étape par étape.")).color(Color32::GRAY));
                    ui.label(RichText::new(tr!("Colle le lien de sa page sur Mobalytics ou Kami-labs.")).color(Color32::GRAY));
                    ui.add_space(10.0);
                    add = ui.button(RichText::new(tr!("Ajouter un build…")).color(ui::GOLD)).clicked();
                });
            });
            if add {
                self.add_build.open = true;
                self.add_build.message = None;
            }
            return;
        }
        let level = coach.level();
        let key = format!("{}:{}", self.builds.active().map(|b| b.id.as_str()).unwrap_or(""), shown);
        if let Some(b) = self.builds.active() {
            self.tree.use_overrides_file(self.builds.dir_of(&b.id).join("attributes.json"));
        }
        let gem_infos = self.gem_infos.get();
        let (tab, assets, tree, icons, weapon_set) = (self.tab, &mut self.assets, &mut self.tree, &mut self.passive_icons, &mut self.gear.set);
        let shown = shown.min(coach.steps.len() - 1);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            // Set d'armes de l'arbre, partagé avec l'écran d'équipement (l'étape se choisit dans la
            // barre du haut). Style doré pour le sélecteur seulement.
            if tab == views::Tab::Passives && coach.steps.iter().any(|s| !s.passive_sets.is_empty()) {
                ui.horizontal(|ui| {
                    ui::gold_widgets(ui);
                    let h = ui.spacing().interact_size.y + 4.0;
                    ui.label(RichText::new(tr!("Set d'armes")).color(ui::GOLD_DIM));
                    let size = ui::set_tabs_size(ui, h, [false; 2]);
                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let mut set = (*weapon_set).max(1);
                    if ui::set_tabs(ui, r.center(), h, "talent_set", &mut set, [false; 2], true) {
                        *weapon_set = set;
                    }
                });
                ui.add_space(6.0);
            }
            let step = &coach.steps[shown];
            let prev = shown.checked_sub(1).map(|i| &coach.steps[i]);
            match tab {
                views::Tab::Skills => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| views::skills(ui, step, prev, level, assets, gem_infos.as_deref()));
                }
                views::Tab::Passives => {
                    let r = ui.available_rect_before_wrap();
                    tree.show(ui, r, step, prev, coach.catalog(), icons, key, (*weapon_set).max(1), &tree_jewels);
                }
                views::Tab::Equipment | views::Tab::Stats | views::Tab::Atlas | views::Tab::Guide => {}
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
                (uniques::missing_by_slot(step, &self.gear, base), guide::step_text(&step.label))
            }
            None => (vec![], String::new()),
        };
        // Pour l'infobulle de l'étoile : la fiche de chaque unique attendu et son prix au marché.
        let names: Vec<String> = wanted.iter().map(|(_, n)| n.clone()).collect();
        let star_info: HashMap<String, ui::WantedUnique> = names.into_iter().map(|n| {
            let found = {
                let prices = self.prices.data.lock().unwrap();
                prices.as_ref().and_then(|p| p.unique_by_name(&n).map(|(_, div, listings)| (p.to_ex(div), listings)))
            };
            let (price, basis) = match found {
                Some((ex, listings)) => (
                    (format!("≈ {}", self.fmt_price(ex)), Color32::from_rgb(225, 220, 210)),
                    Some(if listings < prices::MIN_LISTINGS { tr!("{n} annonces seulement, cours de poe.ninja", n = listings) } else { trn!(listings, "{n} annonce, cours de poe.ninja", "{n} annonces, cours de poe.ninja") }),
                ),
                None => ((tr!("pas de prix au marché").to_string(), Color32::GRAY), None),
            };
            let view = self.unique_view(&n);
            (n, ui::WantedUnique { view, price, basis })
        }).collect();
        let has_star = |slot: Slot| wanted.iter().any(|(s, _)| *s == slot);
        let mut open_detail = None;
        let mut clicked = None;
        let mut star = |ui: &mut egui::Ui, rect: egui::Rect, slot: Slot| {
            let names: Vec<&ui::WantedUnique> = wanted.iter().filter(|(s, _)| *s == slot).filter_map(|(_, n)| star_info.get(n)).collect();
            if !names.is_empty() && ui::wanted_star(ui, rect, slot, &names, &step_label) {
                open_detail = Some(slot);
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
                    if ui::blocked_slot(ui, rect, slot, ui::Blocked::ByTwoHander(weapon_tex.as_ref()), framed, has_star(slot)) {
                        clicked = Some(slot);
                    }
                    star(ui, rect, slot);
                    continue;
                }
                let charm_index = gear::CHARMS.iter().position(|c| *c == slot);
                if let (Some(i), Some(open)) = (charm_index, charm_slots) {
                    if i >= open {
                        if ui::blocked_slot(ui, rect, slot, ui::Blocked::ClosedCharm(blocker.as_ref()), framed, false) {
                            clicked = Some(slot);
                        }
                        continue;
                    }
                }
            }
            // Pièce survolée : sa colonne du détail (calculée pour celle-là seulement).
            let worn = match &item {
                Some(it) if ui.rect_contains_pointer(rect) => Some(self.worn_view(slot, it, card.as_ref())),
                _ => None,
            };
            match ui::slot(ui, rect, slot, item.as_ref(), tex.as_ref(), glow.get(&slot).copied(), framed, has_star(slot), sockets.as_ref(), card.as_ref(), ghost.as_ref(), worn.as_ref()) {
                ui::SlotAction::Clear => to_clear = Some(slot),
                ui::SlotAction::Open => clicked = Some(slot),
                ui::SlotAction::None => {}
            }
            star(ui, rect, slot);

        }
        // Set inactif : ses armes en petit aux extrémités (comme en jeu), avec leur propre cadre.
        let other = if set == 2 { 1 } else { 2 };
        let other_two_handed = self.gear.item(Slot::Weapon.in_set(other)).is_some_and(|w| gear::kind_of(&w.item_class) == Some(gear::Kind::TwoHanded));
        for base in [Slot::Weapon, Slot::Offhand] {
            let slot = base.in_set(other);
            let rect = ui::place(area, scale, ui::mini_layout(base));
            let item = self.gear.item(slot);
            if item.is_none() && base == Slot::Offhand && other_two_handed {
                let weapon_tex = self.gear.item(Slot::Weapon.in_set(other)).and_then(|w| self.assets.texture(&w));
                if ui::blocked_slot(ui, rect, slot, ui::Blocked::ByTwoHander(weapon_tex.as_ref()), true, has_star(slot)) {
                    clicked = Some(slot);
                }
                star(ui, rect, slot);
                continue;
            }
            let tex = item.as_ref().and_then(|it| self.assets.texture(it));
            let card = self.build_card(item.as_ref(), slot);
            let worn = match &item {
                Some(it) if ui.rect_contains_pointer(rect) => Some(self.worn_view(slot, it, card.as_ref())),
                _ => None,
            };
            // Runes en petit : les châsses suivent la taille de la case.
            let sockets = item.as_ref().and_then(|it| self.socket_view(it, slot));
            match ui::slot(ui, rect, slot, item.as_ref(), tex.as_ref(), glow.get(&slot).copied(), true, has_star(slot), sockets.as_ref(), card.as_ref(), None, worn.as_ref()) {
                ui::SlotAction::Clear => to_clear = Some(slot),
                ui::SlotAction::Open => clicked = Some(slot),
                ui::SlotAction::None => {}
            }
            star(ui, rect, slot);
        }
        self.gear.set = new_set;
        if let Some(slot) = open_detail.or(clicked) {
            self.slot_detail = Some(slot);
            self.detail_mode = DetailMode::Worn;
        }
        if let Some(slot) = to_clear {
            self.gear.clear(slot);
            let _ = self.gear.save();
            self.say(tr!("{slot} n'est plus suivi", slot = slot.label()));
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
                Some(trade::Estimate::Done { total: 0, .. }) => (tr!("sans valeur").to_string(), Color32::GRAY),
                Some(trade::Estimate::Done { .. }) => (tr!("incertain").to_string(), Color32::GRAY),
                _ => (tr!("prix inconnu").to_string(), Color32::from_gray(110)),
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
            i18n::decimal(format!("{v:.1}"))
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
                i18n::decimal(format!("{v:.1}"))
            } else if v >= 0.01 {
                i18n::decimal(format!("{v:.2}"))
            } else {
                i18n::decimal("< 0.01".into())
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
            ui.label(RichText::new(tr!("Ctrl+C sur un objet en jeu pour le diagnostiquer : il s'ajoutera ici.")).color(Color32::GRAY));
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
                let shown_name = gametext::name(&it.name);
                let inline = text_w(&shown_name, egui::TextStyle::Body)
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
                        ui.add(egui::Label::new(RichText::new(shown_name.as_ref()).color(ui::rarity_color(it.rarity))).truncate());
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
                            None => estimate_rect = Some(ui::action_button(ui, tr!("💰  Estimer")).rect),
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
                _ if self.needs_estimate(it) => tr!("pas encore estimé : bouton « Estimer »").into(),
                Err("…") => tr!("en cours…").into(),
                _ if matches!(
                    self.trade.get(it),
                    Some(trade::Estimate::Done { total: 0, .. })
                ) =>
                {
                    tr!("sans valeur : aucune annonce comparable en achat immédiat").into()
                }
                Err("?") => tr!("trop peu d'annonces pour estimer").into(),
                Err(_) => tr!("inconnu").into(),
            };
            let confidence = self.trade.get(it).filter(|_| matches!(self.price_ex(it), Ok((_, true)))).map(|e| e.confidence_pct());
            let resp = resp.on_hover_ui(|ui| {
                ui.set_max_width(480.0);
                ui.spacing_mut().item_spacing.y = 5.0;
                let body = |t: &str| RichText::new(t).size(16.0);
                // Le nom et le type sont déjà sur la ligne : on commence par le verdict.
                if let Some(c) = &comparison {
                    let (text, color) = ui::verdict_style(c.verdict);
                    let against = if c.against_name.is_empty() || c.verdict == compare::Verdict::Same { String::new() } else { tr!(" que {name}", name = c.against_name) };
                    ui.label(body(&format!("{}{against}", text.to_lowercase())).color(color));
                    ui.add_space(6.0);
                }
                let advice = value.label.advice();
                ui.horizontal(|ui| {
                    ui.label(body(tr!("Conseil :")).color(Color32::from_gray(225)).strong());
                    ui.label(body(advice).color(ui::value_color(value.label)).strong());
                });
                ui.horizontal(|ui| {
                    ui.label(body(tr!("Prix estimé :")).color(Color32::from_gray(225)).strong());
                    ui.label(body(&price).color(ui::GOLD));
                });
                if let Some(p) = confidence {
                    ui.horizontal(|ui| {
                        ui.label(body(tr!("Confiance :")).color(Color32::from_gray(225)).strong());
                        ui.label(body(&i18n::percent(p)).color(ui::confidence_color(p)));
                    });
                }
                ui.label(body(&value.summary).color(Color32::LIGHT_GRAY));
                for r in &value.reasons {
                    ui.label(body(&format!("• {r}")).color(Color32::GRAY));
                }
                ui.add_space(6.0);
                ui.label(RichText::new(tr!("clic : ouvrir le diagnostic · clic droit : retirer")).size(14.0).color(Color32::from_gray(110)).italics());
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
                if ui.button(tr!("Retirer de l'historique")).clicked() {
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
    fn item_preview(&mut self, ui: &mut egui::Ui, it: &Item, verdict: Option<(&'static str, Color32)>, view: Option<&ui::WornView>) {
        // Survol de l'aperçu = fiche complète en infobulle (rien ne se déplie dans le panneau).
        // En-tête commun à toute l'app (voir `ui::item_header`) : avec sa fiche, les mêmes lignes
        // que l'en-tête d'une pièce équipée (défenses, propriétés, niveau requis, implicites).
        // Avec sa fiche : la pièce entière, comme l'infobulle d'une pièce équipée (en-tête, mods,
        // runes), sans survol à faire.
        if let Some(w) = view {
            // Agrandie selon la place : la taille de la fiche à l'échelle 1 (mesurée) et la place
            // laissée par le panneau donnent le facteur, entre 1 et 1,8.
            let natural_id = ui.id().with("diag_natural");
            let natural: Vec2 = ui.data(|d| d.get_temp(natural_id)).unwrap_or(Vec2::new(400.0, 300.0));
            let room = Vec2::new(ui.available_width(), ui.clip_rect().bottom() - ui.cursor().top() - 24.0);
            let scale = (room.x / natural.x).min(room.y / natural.y).clamp(1.0, 1.8);
            // Le texte grossit moins que la place ne le permettrait ; l'image, elle, suit la hauteur.
            let scale = 1.0 + (scale - 1.0) * 0.6;
            let start = ui.cursor().min;
            ui.scope(|ui| {
                // Corps de texte (mods, runes) à la même échelle que l'en-tête.
                for (style, font) in ui.style_mut().text_styles.iter_mut() {
                    if matches!(style, egui::TextStyle::Body | egui::TextStyle::Small) {
                        font.size *= scale;
                    }
                }
                ui.spacing_mut().item_spacing.y *= scale;
                let lines = ui::header_lines_full(&w.weapon, &w.props, &[], &w.implicits, &[], scale);
                ui::item_header_scaled(ui, "diag", w.image.as_ref(), &w.title, w.color, w.after.as_deref(), lines, verdict, scale, true);
                if !w.mods.is_empty() {
                    ui.separator();
                    ui::worn_mods(ui, w);
                }
                ui::worn_runes(ui, w, ui.available_width());
            });
            // Taille à l'échelle 1 : hauteur ramenée, largeur de la ligne la plus longue.
            let h = (ui.cursor().top() - start.y) / scale;
            let w_nat = ui::worn_width(ui, w) + 160.0;
            let fresh = Vec2::new(w_nat, h);
            if (fresh - natural).length() > 1.0 {
                ui.data_mut(|d| d.insert_temp(natural_id, fresh));
                ui.ctx().request_repaint();
            }
            return;
        }
        let base_defences = self.base_defences_text(it);
        // Défenses de base du type, implicites, niveau requis.
        let mut lines: Vec<egui::WidgetText> = Vec::new();
        if let Some(d) = base_defences {
            lines.push(RichText::new(d).color(Color32::GRAY).size(ui::HEADER_LINE_SIZE).into());
        }
        for m in it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k == item::ModKind::Implicit).map(|(m, _)| m) {
            lines.push(RichText::new(gametext::line(m)).color(Color32::from_rgb(136, 136, 255)).size(ui::HEADER_LINE_SIZE).into());
        }
        if it.req_level > 0 {
            let mut job = egui::text::LayoutJob::default();
            let font = egui::FontId::proportional(ui::HEADER_LINE_SIZE);
            job.append(tr!("Niveau requis "), 0.0, egui::TextFormat::simple(font.clone(), Color32::GRAY));
            let color = if it.req_level > self.char_level() { ui::SOCKET_WRONG } else { Color32::from_gray(225) };
            job.append(&it.req_level.to_string(), 0.0, egui::TextFormat::simple(font, color));
            lines.push(job.into());
        }
        let tex = self.assets.texture(it);
        let (title, after) = ui::item_title(it);
        ui::item_header(ui, "diag", tex.as_ref(), title, ui::rarity_color(it.rarity), after, lines.into_iter().map(Into::into).collect(), verdict);
        // Objet qui ne se porte pas (monnaie, omen, gemme…) : son texte, tel quel.
        let text: Vec<&String> = it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k != item::ModKind::Implicit).map(|(m, _)| m).collect();
        if !text.is_empty() {
            ui.separator();
            let text: Vec<String> = text.into_iter().cloned().collect();
            for (_, m) in gametext::lines(&text) {
                ui.label(RichText::new(m).color(Color32::from_gray(225)));
            }
        }
    }

    fn diagnostic(&mut self, ui: &mut egui::Ui, comparison: Option<&compare::Comparison>) -> Option<(String, Color32)> {
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
            (Some(_), _) => Some((tkey!("★ UNIQUE DU BUILD"), ui::rarity_color(item::Rarity::Unique))),
            (None, Some(c)) => Some(ui::verdict_style(c.verdict)),
            (None, None) => None,
        };
        // Emplacements jugés : celui de la pièce, ou chacun de ceux d'un anneau ou d'un charme, avec
        // pour chacun la pièce portée, la réponse et les scores.
        let multi = self.multi_slots(&it);
        let candidates: Vec<Slot> = if multi.is_empty() {
            comparison.and_then(|c| c.against.first().copied()).or_else(|| self.target_slot(&it)).into_iter().collect()
        } else {
            multi.clone()
        };
        let per_slot: Vec<SlotDiag> = candidates
            .iter()
            .map(|&s| {
                let at = if multi.is_empty() { None } else { self.compare_at(&it, Some(s)) };
                let c = if multi.is_empty() { comparison } else { at.as_ref() };
                let card = self.build_card(Some(&it), s);
                let it_score = self.worn_view(s, &it, card.as_ref()).score;
                let worn = self.gear.item(s);
                let worn_score = worn.as_ref().and_then(|w| {
                    let worn_card = self.build_card(Some(w), s);
                    self.worn_view(s, w, worn_card.as_ref()).score
                });
                let gap = match (it_score, worn_score) {
                    (Some(a), Some(w)) if worn.is_some() => Some(a as i64 - w as i64),
                    _ => None,
                };
                SlotDiag { slot: s, worn: worn.is_some(), build: card.is_some(), it_score, gap, answer: Self::diag_verdict(c, &recommended, it_score, worn_score) }
            })
            .collect();
        // La meilleure place : la meilleure réponse, puis le plus grand écart, puis le score.
        let rank = |a: &str| if a.contains("UNIQUE") { 4 } else if a.contains("MIEUX") { 3 } else if a.contains("VOIR") { 2 } else { 1 };
        let best = per_slot.iter().max_by_key(|d| (rank(d.answer.0), d.gap.unwrap_or(i64::MIN), d.it_score.unwrap_or(0))).cloned();
        // Emplacement montré, la pièce portée là, ce qu'y vise le build.
        let slot = best.as_ref().map(|b| b.slot);
        let card = slot.and_then(|s| self.build_card(Some(&it), s));
        // L'objet tel quel (mods visés en blanc, les autres en gris) face au build.
        let view = slot.map(|s| self.worn_view(s, &it, card.as_ref()));
        // Bande de titre à la hauteur du badge et du prix (dessinés à part en haut à droite) : la
        // fiche commence dessous et ne passe jamais sous eux.
        // Badge de la réponse (voir la fin) : le titre s'arrête avant lui, quitte à passer à la ligne.
        let badge = best.as_ref().filter(|_| comparison.is_some()).map(|b| {
            let side = b.slot.side().filter(|_| per_slot.len() > 1).map(|s| format!(" · {s}")).unwrap_or_default();
            let gap = b.gap.map(|d| format!(" ({}{})", if d > 0 { "+" } else if d < 0 { "−" } else { "" }, i18n::percent(d.abs()))).unwrap_or_default();
            (format!("{}{side}{gap}", i18n::tr(b.answer.0)), b.answer.1)
        });
        let badge_w = badge.as_ref().map_or(0.0, |(t, _)| ui.fonts(|f| f.layout_no_wrap(t.clone(), egui::FontId::proportional(17.0), Color32::WHITE).size().x) + 36.0);
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 72.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_height(72.0);
            ui.add_space(4.0);
            ui.set_max_width((ui.available_width() - badge_w).max(120.0));
            ui.add(egui::Label::new(RichText::new(if slot.is_some() { tr!("PIÈCE DIAGNOSTIQUÉE") } else { tr!("OBJET DIAGNOSTIQUÉ") }).color(ui::GOLD).size(15.5).strong()).wrap());
            let place = slot.map(|s| s.label().to_string()).unwrap_or_else(|| tr!("ne se porte pas").into());
            ui.label(RichText::new(place).color(Color32::GRAY));
        });
        ui.separator();
        self.item_preview(ui, &it, None, view.as_ref());
        ui.add_space(6.0);
        // Les bandeaux « comparer » sont en bas, au-dessus de l'estimation (voir `diag_bands`).
        self.diag_bands = (!per_slot.is_empty()).then(|| DiagBands { slots: per_slot.clone() });
        ui.add_space(8.0);
        // Le badge de l'en-tête porte la réponse (mieux, moins bien, à voir).
        let _ = verdict;
        // Pas de verdict pour un objet qui ne se porte pas : seul son prix compte. Sinon la meilleure
        // place s'il y en a plusieurs, et l'écart de score face à ta pièce (comme le bandeau
        // « comparer à l'équipé »).
        badge
    }

    /// Réponse du diagnostic, le badge de l'en-tête : faut-il l'équiper ? Mieux que ta pièce et
    /// pas plus loin du build (mieux), l'inverse (moins bien), ou les deux qui se contredisent.
    fn diag_verdict(comparison: Option<&compare::Comparison>, recommended: &Option<(Vec<String>, bool)>, it_score: Option<u32>, worn_score: Option<u32>) -> (&'static str, Color32) {
        if recommended.as_ref().is_some_and(|(_, now)| *now) {
            return (tkey!("★ UNIQUE DU BUILD"), ui::rarity_color(item::Rarity::Unique));
        }
        let Some(c) = comparison else { return (tkey!("▼ MOINS BIEN"), ui::SOCKET_WRONG) };
        // Base qui ne convient pas au build : moins bien, quoi qu'il arrive.
        if c.deltas.iter().any(|d| d.headline && d.text.starts_with(tr!("pas pour ce build"))) {
            return (tkey!("▼ MOINS BIEN"), ui::SOCKET_WRONG);
        }
        let better = match c.verdict {
            compare::Verdict::Better => Some(true),
            compare::Verdict::Worse => Some(false),
            _ => None,
        };
        let closer = match (it_score, worn_score) {
            (Some(a), Some(b)) => Some(a >= b),
            (Some(a), None) => Some(a > 0),
            _ => None,
        };
        match (better, closer) {
            (Some(true), Some(true) | None) => (tkey!("▲ MIEUX"), ui::SOCKET_GOOD),
            (Some(false), Some(false) | None) | (None, Some(false)) => (tkey!("▼ MOINS BIEN"), ui::SOCKET_WRONG),
            _ => (tkey!("? À VOIR"), ui::EMPTY_SOCKET),
        }
    }

    /// Sous l'en-tête du diagnostic : la pièce portée et la pièce visée par le build, chacune
    /// comparée à la pièce diagnostiquée, prêtes à afficher.
    #[allow(clippy::too_many_arguments)]
    fn diag_sides(
        &mut self,
        it: &Item,
        comparison: Option<&compare::Comparison>,
        card: Option<&ui::BuildCard>,
        worn: Option<&Item>,
        recommended: &Option<(Vec<String>, bool)>,
        judged: Option<&str>,
        worn_score: Option<u32>,
        it_score: Option<u32>,
    ) -> (DiagSide, Option<DiagSide>) {
        let good = ("✓", ui::SOCKET_GOOD);
        let partial = ("~", ui::EMPTY_SOCKET);
        let missing = ("✗", ui::SOCKET_WRONG);
        // Ta pièce : ce qu'on gagne (+) et ce qu'on perd (✗) en la remplaçant.
        let mut left = DiagSide { score: worn_score, ..Default::default() };
        match (comparison, worn) {
            (Some(c), Some(w)) => {
                let slot = c.against.first().copied().unwrap_or(Slot::Weapon);
                let worn_card = self.build_card(Some(w), slot);
                let view = self.worn_view(slot, w, worn_card.as_ref());
                // Défenses et DPS de ta pièce face à ceux de la pièce diagnostiquée.
                let keys = ["Armour", "Evasion Rating", "Energy Shield"];
                let (mine, new) = (keys.map(|k| w.props.get(k).copied().unwrap_or(0.0)), keys.map(|k| it.props.get(k).copied().unwrap_or(0.0)));
                let mut marks: Vec<Option<(&'static str, Color32)>> = vec![None; view.props.len()];
                if mine.iter().any(|v| *v > 0.0) && !marks.is_empty() {
                    marks[0] = Some(if mine.iter().zip(new).any(|(m, n)| *m > 0.0 && n <= 0.0) {
                        missing
                    } else if mine.iter().zip(new).all(|(m, n)| n >= *m) {
                        ("+", ui::SOCKET_GOOD)
                    } else {
                        partial
                    });
                }
                let mut weapon = view.weapon.clone();
                if let (Some(m), Some(n)) = (compare::weapon_dps(w), compare::weapon_dps(it)) {
                    let mark = if n.total() >= m.total() { ("+", ui::SOCKET_GOOD) } else { missing };
                    // Le DPS total est au bout de la ligne des attaques par seconde (sinon sur sa ligne).
                    if let Some(line) = weapon.iter_mut().find(|l| l.first().is_some_and(|(t, _)| t.starts_with(tr!("Attaques par seconde")))) {
                        line.insert(0, (format!("{} ", mark.0), mark.1));
                    } else if let Some(i) = view.props.iter().position(|p| p.first().is_some_and(|(t, _)| t == "DPS ")) {
                        marks[i] = Some(mark);
                    }
                }
                left.weapon = weapon;
                left.image = view.image.clone();
                left.name = view.title.clone();
                left.name_color = view.color;
                left.after = view.after.clone();
                left.props = view.props.clone();
                left.prop_marks = marks;
                left.implicits = view.implicits.clone();
                // Implicites et compétence de ta pièce : gardés par la pièce diagnostiquée (+), en
                // moins (~) ou perdus (✗).
                let it_implicits: Vec<String> = it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k == item::ModKind::Implicit).map(|(m, _)| m.clone())
                    .chain(it.granted_skills.iter().map(|s| format!("{}{s}", tr!("Octroie l'Aptitude : "))))
                    .collect();
                let kept = ("+", ui::SOCKET_GOOD);
                left.implicit_marks = left
                    .implicits
                    .iter()
                    .map(|l| {
                        if let Some(skill) = l.strip_prefix(tr!("Octroie l'Aptitude : ")) {
                            let same = it_implicits.iter().any(|w| w.strip_prefix(tr!("Octroie l'Aptitude : ")).is_some_and(|n| skill_name(n) == skill_name(skill)));
                            return Some(if same { kept } else { missing });
                        }
                        let (tpl, mine) = trade::template(l);
                        Some(match it_implicits.iter().map(|w| trade::template(w)).find(|(t, _)| *t == tpl) {
                            Some((_, new)) if new.unwrap_or(0.0) < mine.unwrap_or(0.0) => partial,
                            Some(_) => kept,
                            None => missing,
                        })
                    })
                    .collect();
                // Runes de ta pièce : la pièce diagnostiquée porte la même (+), sinon perdue (✗).
                let mut have: Vec<String> = self.socketed(it).map(|s| s.augments).unwrap_or_default();
                left.runes = view.runes.clone();
                for r in &mut left.runes {
                    r.mark = Some(match have.iter().position(|h| *h == r.name) {
                        Some(i) => {
                            have.remove(i);
                            kept
                        }
                        None => missing,
                    });
                }
                // Le constat sur la base et les dégâts de l'arme sont avec les en-têtes.
                left.lines = c
                    .deltas
                    .iter()
                    .filter(|d| !d.headline && !d.dps)
                    .map(|d| match d.sign {
                        1 => (Some(("+", ui::SOCKET_GOOD)), ui::SOCKET_GOOD, d.text.clone()),
                        -1 => (Some(missing), ui::SOCKET_WRONG, d.text.clone()),
                        _ => (Some(("•", Color32::from_gray(115))), Color32::from_gray(115), d.text.clone()),
                    })
                    .collect();
                left.notes = c.notes.clone();
            }
            (Some(c), None) => {
                left.notes = if c.notes.is_empty() { vec![tr!("Pas de pièce suivie à cet emplacement.").into()] } else { c.notes.clone() };
            }
            (None, _) => left.notes = vec![tr!("Ne se porte pas : pas de comparaison.").into()],
        }
        // Pièce du build : ce qu'il vise, et si la pièce diagnostiquée l'a (✓ ~ ✗).
        if recommended.is_none() && card.is_none() {
            return (left, None);
        }
        let _ = judged;
        let mut right = DiagSide { score: it_score, ..Default::default() };
        if let Some((steps, current)) = recommended {
            right.notes.push(if *current { tr!("unique recommandé à cette étape").to_string() } else { tr!("unique recommandé à : {steps}", steps = steps.iter().map(|s| guide::step_text(s)).collect::<Vec<_>>().join(", ")) });
        }
        let Some(b) = card else { return (left, Some(right)) };
        let unique_text = b.unique.as_ref().and_then(|u| self.coach.as_ref()?.catalog().uniques.get(u).cloned());
        let base = unique_text.as_ref().map(|t| t.base.clone()).unwrap_or_else(|| b.base.clone());
        right.image = b.image.clone();
        right.name = b.unique.clone().unwrap_or_else(|| b.base.clone());
        right.name_color = ui::rarity_color(if b.unique.is_some() { item::Rarity::Unique } else { item::Rarity::Rare });
        right.after = b.unique.as_ref().map(|_| base.clone()).filter(|s| !s.is_empty());
        if let Some(d) = b.base_defences {
            let def = defence_parts(d, [false; 3]);
            if !def.is_empty() {
                let have = ["Armour", "Evasion Rating", "Energy Shield"].map(|k| it.props.get(k).copied().unwrap_or(0.0));
                right.prop_marks.push(Some(if d.iter().zip(have).any(|(w, h)| *w > 0.0 && h <= 0.0) {
                    missing
                } else if d.iter().zip(have).all(|(w, h)| h >= *w) {
                    good
                } else {
                    partial
                }));
                right.props.push(def);
            }
        }
        if let Some(level) = self.augments.get().and_then(|c| c.req_levels.get(&base).copied()) {
            right.props.push(ui::req_level_parts(level, self.char_level()));
            right.prop_marks.push(None);
        }
        right.implicits = unique_text.as_ref().map(|t| t.implicits.clone()).unwrap_or_else(|| b.implicits.clone());
        let it_implicits: Vec<String> = it.mods.iter().zip(&it.mod_kinds).filter(|(_, k)| **k == item::ModKind::Implicit).map(|(m, _)| m.clone())
            .chain(it.granted_skills.iter().map(|s| format!("{}{s}", tr!("Octroie l'Aptitude : "))))
            .collect();
        right.implicit_marks = right
            .implicits
            .iter()
            .map(|l| {
                if let Some(skill) = l.strip_prefix(tr!("Octroie l'Aptitude : ")) {
                    let same = it_implicits.iter().any(|w| w.strip_prefix(tr!("Octroie l'Aptitude : ")).is_some_and(|n| n.contains(skill)));
                    return Some(if same { good } else { missing });
                }
                let (tpl, want) = trade::template(l);
                Some(match it_implicits.iter().map(|w| trade::template(w)).find(|(t, _)| *t == tpl) {
                    Some((_, have)) if have.unwrap_or(0.0) < want.unwrap_or(0.0) => partial,
                    Some(_) => good,
                    None => missing,
                })
            })
            .collect();
        right.lines = match unique_text.as_ref().map(|t| t.mods.clone()) {
            Some(mods) => mods
                .iter()
                .zip(unique_matches(it, &mods))
                .map(|(m, part)| {
                    let (mark, c) = match_mark(part);
                    (Some((mark, c)), c, m.clone())
                })
                .collect(),
            None => b
                .lines
                .iter()
                .map(|(l, have)| {
                    let (mark, c) = match have {
                        compare::Have::Full(_) => good,
                        compare::Have::Partial(_) => partial,
                        _ => missing,
                    };
                    (Some((mark, c)), c, l.clone())
                })
                .collect(),
        };
        // Runes conseillées, posées ou non sur la pièce diagnostiquée.
        let mut have: Vec<String> = self.socketed(it).map(|s| s.augments).unwrap_or_default();
        right.runes = b.runes.clone();
        for r in &mut right.runes {
            r.mark = Some(match have.iter().position(|h| *h == r.name) {
                Some(i) => {
                    have.remove(i);
                    good
                }
                None => missing,
            });
        }
        (left, Some(right))
    }

    /// « Base : Energy Shield 40 » : défenses du type de base d'une pièce (un objet magique
    /// cache ce type dans son nom), s'il en a.
    fn base_defences_text(&self, it: &Item) -> Option<String> {
        let base = self.assets.index.lock().unwrap().as_ref().and_then(|i| i.base_of(it)).unwrap_or_else(|| it.base.clone());
        let d = self.augments.get()?.defences.get(&base).copied()?;
        let parts: Vec<String> = [tr!("Armure"), tr!("Score d'Évasion"), tr!("Bouclier d'énergie")].iter().zip(d).filter(|(_, v)| *v > 0.0).map(|(n, v)| format!("{n} {v:.0}")).collect();
        Some(tr!("Base : {defences}", defences = parts.join(" · ")))
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
    /// Onglet Maps : chiffres de l'endgame du personnage, par tier, puis l'historique des maps.
    fn maps_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let (filter, tier) = (self.maps_filter, self.maps_tier);
        let (mut pick_filter, mut pick_tier) = (filter, tier);
        let Some(book) = self.coach.as_mut().and_then(|c| c.maps()) else { return };
        let tier_key = |r: &mapping::Run| if r.kind == mapping::RunKind::Boss { 0 } else { r.level };
        let shown = |r: &&mapping::Run| filter.is_none_or(|g| r.group == g) && tier.is_none_or(|t| tier_key(r) == t);
        // Chiffres clés sur les maps ; sur les arènes quand on a choisi les boss.
        let kind = if tier == Some(0) { mapping::RunKind::Boss } else { mapping::RunKind::Map };
        let maps: Vec<&mapping::Run> = book.runs.iter().filter(|r| r.kind == kind).filter(shown).collect();
        // Tiers rencontrés, du plus haut au plus bas, les boss à la fin.
        let mut tiers: Vec<(u32, String)> = Vec::new();
        for r in &book.runs {
            if !tiers.iter().any(|(k, _)| *k == tier_key(r)) {
                tiers.push((tier_key(r), if r.kind == mapping::RunKind::Boss { tr!("Boss").to_string() } else { run_tier(r) }));
            }
        }
        tiers.sort_by_key(|(k, _)| std::cmp::Reverse(*k));
        let dim = Color32::GRAY;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 }).show(ui, |ui| {
                let count = book.runs.iter().filter(shown).count();
                ui::section_header(ui, tr!("MAPS"), Some(count), |ui| {
                    // À la taille des listes du personnage et du build.
                    ui::gold_widgets(ui);
                    let group_label = |g: Option<bool>| match g {
                        None => tr!("Solo et groupe"),
                        Some(false) => tr!("Solo"),
                        Some(true) => tr!("En groupe"),
                    };
                    // Le tier à gauche, solo/groupe à droite, chacun dans une case de même taille
                    // centrée sur la ligne du titre (sinon la seconde liste descend d'un cran).
                    let row = ui.available_rect_before_wrap();
                    let h = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
                    let cell = |right: f32| egui::Rect::from_min_max(egui::pos2(right - 160.0, row.center().y - h / 2.0), egui::pos2(right, row.center().y + h / 2.0));
                    let center = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(cell(row.right())).layout(center), |ui| {
                        egui::ComboBox::from_id_salt("maps_group")
                            .width(150.0)
                            .selected_text(RichText::new(group_label(filter)).color(ui::GOLD))
                            .show_ui(ui, |ui| {
                                for g in [None, Some(false), Some(true)] {
                                    ui.selectable_value(&mut pick_filter, g, group_label(g));
                                }
                            })
                            .response
                            .on_hover_text(tr!("Les moyennes ne se comparent qu'entre maps du même genre"));
                    });
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(cell(row.right() - 168.0)).layout(center), |ui| {
                        let tier_label = |t: Option<u32>| t.and_then(|t| tiers.iter().find(|(k, _)| *k == t)).map_or(tr!("Tous les tiers").to_string(), |(_, l)| l.clone());
                        egui::ComboBox::from_id_salt("maps_tier")
                            .width(150.0)
                            .selected_text(RichText::new(tier_label(tier)).color(ui::GOLD))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut pick_tier, None, tr!("Tous les tiers"));
                                for (k, l) in &tiers {
                                    ui.selectable_value(&mut pick_tier, Some(*k), l);
                                }
                            });
                    });
                });
                if let Some(w) = book.pending_waystone() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr!("PIERRE DE TÉLÉPORTATION PRÊTE")).color(ui::GOLD_DIM).size(13.5).strong());
                        ui.label(RichText::new(format!("T{} · {}", w.tier, w.name)).color(ui::GOLD));
                        if w.corrupted {
                            ui.label(RichText::new(tr!("corrompu")).color(dim).size(13.5));
                        }
                    });
                    waystone_lines(ui, w);
                    ui.label(RichText::new(tr!("Il sera associé à la prochaine map ouverte.")).color(dim).size(13.5).italics());
                    ui.add_space(10.0);
                }
                if book.runs.is_empty() {
                    ui.label(RichText::new(tr!("Les maps s'afficheront ici dès ta première entrée en map : durée, morts, retours en ville et contenus visités.")).color(dim));
                    return;
                }
                // Chiffres clés, sur le temps de jeu dans les maps (hors AFK et hors repaire).
                if !maps.is_empty() {
                    let play: i64 = maps.iter().map(|r| r.active_secs()).sum();
                    let deaths: u32 = maps.iter().map(|r| r.deaths).sum();
                    let n = maps.len() as f64;
                    let per_hour = if play > 0 { n * 3600.0 / play as f64 } else { 0.0 };
                    let (each, per, in_maps) = if kind == mapping::RunKind::Boss { (tr!("par boss en moyenne"), tr!("boss par heure"), tr!("de jeu en boss")) } else { (tr!("par map en moyenne"), tr!("maps par heure"), tr!("de jeu en maps")) };
                    let figures = [
                        (duration_text(play / maps.len() as i64), each),
                        (i18n::decimal(format!("{per_hour:.1}")), per),
                        (i18n::decimal(format!("{:.2}", deaths as f64 / n)), if kind == mapping::RunKind::Boss { tr!("morts par boss") } else { tr!("morts par map") }),
                        (duration_text(play), in_maps),
                    ];
                    // Une carte par chiffre, centrée ; 2 par ligne quand la place manque pour 4.
                    let gap = 10.0;
                    let width = ui.available_width();
                    let per_row = if width >= 4.0 * 160.0 + 3.0 * gap { 4 } else { 2 };
                    let card = Vec2::new((width - gap * (per_row - 1) as f32) / per_row as f32, 78.0);
                    for row in figures.chunks(per_row) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for (value, label) in row {
                                let (rect, _) = ui.allocate_exact_size(card, egui::Sense::hover());
                                let p = ui.painter();
                                p.rect(rect, 4.0, ui::CARD_FILL, egui::Stroke::new(1.0_f32, Color32::from_gray(48)), egui::StrokeKind::Inside);
                                let c = rect.center();
                                p.text(c - Vec2::new(0.0, 10.0), egui::Align2::CENTER_CENTER, value, egui::FontId::proportional(28.0), Color32::from_gray(225));
                                p.text(c + Vec2::new(0.0, 20.0), egui::Align2::CENTER_CENTER, *label, egui::FontId::proportional(13.5), dim);
                            }
                        });
                        ui.add_space(gap - ui.spacing().item_spacing.y);
                    }
                    // Mods de waystone rencontrés sur au moins deux maps : les plus mortels en haut.
                    let mut by_mod: BTreeMap<String, (u32, i64, u32)> = BTreeMap::new();
                    for r in &maps {
                        for m in r.waystone.iter().flat_map(|w| &w.mods) {
                            let e = by_mod.entry(mapping::mod_pattern(m)).or_default();
                            *e = (e.0 + 1, e.1 + r.active_secs(), e.2 + r.deaths);
                        }
                    }
                    let mut mods: Vec<_> = by_mod.into_iter().filter(|(_, (n, _, _))| *n >= 2).collect();
                    mods.sort_by(|a, b| (b.1.2 as f64 / b.1.0 as f64).total_cmp(&(a.1.2 as f64 / a.1.0 as f64)).then(b.1.0.cmp(&a.1.0)));
                    if !mods.is_empty() {
                        ui.add_space(22.0);
                        ui.label(RichText::new(tr!("MODS DES PIERRES DE TÉLÉPORTATION")).color(ui::GOLD_DIM).size(13.0).strong());
                        egui::Grid::new("map_mods").num_columns(4).spacing([24.0, 6.0]).min_col_width(110.0).show(ui, |ui| {
                            // Le mod en dernier : il prend la place qui reste.
                            for h in [tr!("Maps"), tr!("Temps moyen"), tr!("Morts par map"), tr!("Mod")] {
                                ui.label(RichText::new(h).color(dim).size(13.5));
                            }
                            ui.end_row();
                            for (m, (count, secs, deaths)) in mods.iter().take(10) {
                                ui.label(RichText::new(count.to_string()).color(Color32::LIGHT_GRAY).size(15.5));
                                ui.label(RichText::new(duration_text(secs / *count as i64)).color(Color32::LIGHT_GRAY).size(15.5));
                                ui.label(RichText::new(i18n::decimal(format!("{:.2}", *deaths as f64 / *count as f64))).color(Color32::LIGHT_GRAY).size(15.5));
                                ui.label(RichText::new(gametext::line(m)).color(Color32::from_rgb(140, 150, 230)).size(15.5));
                                ui.end_row();
                            }
                        });
                    }
                }
                // Historique : la plus récente en haut, bandes alternées comme l'historique des objets.
                ui.add_space(22.0);
                ui.label(RichText::new(tr!("HISTORIQUE")).color(ui::GOLD_DIM).size(13.0).strong());
                ui.add_space(2.0);
                let out = egui::ScrollArea::vertical().id_salt("maps_history").auto_shrink([false, false]).show(ui, |ui| {
                    // Colonne des mods seulement si une map affichée a un waystone.
                    let any_mods = book.runs.iter().filter(shown).any(|r| r.waystone.is_some());
                    for (i, run) in book.runs.iter().rev().filter(shown).enumerate() {
                        if i > 0 {
                            ui.add_space(-ui.spacing().item_spacing.y);
                        }
                        let band = ui.painter().with_clip_rect(ui.clip_rect().expand2(Vec2::new(8.0, 0.0)));
                        let stripe = band.add(egui::Shape::Noop);
                        // Une ligne : le tier et le nom à gauche, puis en colonnes alignées sur la
                        // droite la date, les mods, les morts, les retours en ville et la durée.
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::hover());
                        let icons = &mut self.passive_icons;
                        // Colonnes depuis la droite : durée, retours, morts, mods, date, chacune à la
                        // largeur de son texte le plus long, séparées d'un même écart. Quand la place
                        // manque pour le nom, les morts et retours disparaissent d'abord, puis les mods,
                        // puis la date ; ce qui est masqué passe dans l'infobulle de la durée.
                        const GAP: f32 = 18.0;
                        let text_w = |t: &str, size: f32| ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(size), Color32::WHITE).size().x.ceil();
                        let dur_w = text_w("00 min 00 s", 18.0);
                        let count_w = 22.0 + 3.0 + text_w("00", 15.0);
                        let mods_w = if any_mods { text_w(&trn!(10, "{n} mod", "{n} mods"), 13.5) } else { 0.0 };
                        let date_w = text_w("29 sept. 23:59", 13.5);
                        // Colonnes gardées (morts et retours, mods, date) : on retire dans cet ordre.
                        let optional = [2.0 * (count_w + GAP), if any_mods { mods_w + GAP } else { 0.0 }, date_w + GAP];
                        let mut keep = [true; 3];
                        for i in 0..3 {
                            let need: f32 = dur_w + optional.iter().zip(&keep).filter(|(_, k)| **k).map(|(w, _)| w).sum::<f32>();
                            if rect.width() - need >= 200.0 {
                                break;
                            }
                            keep[i] = false;
                        }
                        let [counts, mods, date] = keep;
                        let mut right = rect.right();
                        let mut take = |w: f32| {
                            let r = egui::Rect::from_min_max(egui::pos2(right - w, rect.top()), egui::pos2(right, rect.bottom()));
                            right -= w + GAP;
                            r
                        };
                        let col = |ui: &mut egui::Ui, r: egui::Rect, layout: egui::Layout, add: &mut dyn FnMut(&mut egui::Ui)| {
                            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r).layout(layout), |ui| add(ui));
                        };
                        let ltr = egui::Layout::left_to_right(egui::Align::Center);
                        let deaths_text = trn!(run.deaths, "{n} mort", "{n} morts");
                        let returns_text = trn!(run.returns(), "{n} retour en ville", "{n} retours en ville");
                        let mods_text = run.waystone.as_ref().map(|w| trn!(w.mods.len(), "{n} mod", "{n} mods"));
                        col(ui, take(dur_w), egui::Layout::right_to_left(egui::Align::Center), &mut |ui| {
                            ui.label(RichText::new(duration_text(run.active_secs())).color(Color32::from_gray(225)).size(18.0).strong());
                        });
                        if counts {
                            col(ui, take(count_w), ltr, &mut |ui| icon_count(ui, icons, PORTAL_ICON, run.returns(), &returns_text, Color32::from_rgb(110, 150, 255)));
                            col(ui, take(count_w), ltr, &mut |ui| icon_count(ui, icons, TOMB_ICON, run.deaths, &deaths_text, Color32::from_rgb(225, 90, 80)));
                        }
                        if mods && any_mods {
                            let r = take(mods_w);
                            if let Some(m) = &mods_text {
                                col(ui, r, ltr, &mut |ui| {
                                    ui.label(RichText::new(m).color(dim).size(13.5));
                                });
                            }
                        }
                        if date {
                            col(ui, take(date_w), ltr, &mut |ui| {
                                ui.label(RichText::new(date_text(run.start)).color(dim).size(13.5));
                            });
                        }
                        // Le reste de la largeur : tier, nom et solo/groupe.
                        let left = egui::Rect::from_min_max(rect.min, egui::pos2(right, rect.bottom()));
                        col(ui, left, ltr, &mut |ui| {
                            ui.allocate_ui_with_layout(Vec2::new(38.0, 44.0), ltr, |ui| {
                                tier_image(ui, icons, run, 34.0, dim);
                            });
                            // Le nom tronqué laisse la place à solo/groupe juste derrière lui.
                            let group = group_label(run);
                            let group_w = ui.painter().layout_no_wrap(group.text().to_string(), egui::FontId::proportional(13.5), Color32::GRAY).size().x;
                            let name_w = (ui.available_width() - group_w - ui.spacing().item_spacing.x).max(0.0);
                            ui.scope(|ui| {
                                ui.set_max_width(name_w);
                                ui.add(egui::Label::new(RichText::new(run_name(run)).color(Color32::from_gray(220)).size(16.5)).truncate());
                            });
                            ui.label(group);
                        });
                        let row = egui::InnerResponse::new((), ui.interact(rect, ui.id().with(("map_line", i)), egui::Sense::hover()));
                        if i % 2 == 1 {
                            band.set(stripe, egui::Shape::rect_filled(row.response.rect.expand2(Vec2::new(8.0, 0.0)), 0.0, Color32::from_rgb(21, 20, 18)));
                        }
                        // Une seule infobulle pour toute la ligne : tier, date, morts, retours, waystone.
                        row.response.on_hover_ui(|ui| {
                            ui.set_max_width(420.0);
                            ui.label(RichText::new(format!("{} · {}", run_tier(run), run_name(run))).color(ui::GOLD));
                            ui.label(RichText::new(date_text(run.start)).color(Color32::LIGHT_GRAY).size(13.5));
                            ui.label(RichText::new(format!("{} · {}", deaths_text, returns_text)).color(Color32::LIGHT_GRAY).size(13.5));
                            ui.label(RichText::new(time_note(run)).color(Color32::GRAY).size(13.5).italics());
                            if let Some(w) = &run.waystone {
                                ui.add_space(4.0);
                                ui.label(RichText::new(tr!("Pierre de téléportation T{tier} · {name}{corrupted}", tier = w.tier, name = w.name, corrupted = if w.corrupted { tr!(" · corrompu") } else { "" })).color(ui::GOLD));
                                waystone_lines(ui, w);
                            }
                        });
                    }
                });
                ui::fade_scroll(ui, &out, true);
            });
        });
        (self.maps_filter, self.maps_tier) = (pick_filter, pick_tier);
    }

    /// Onglet Boss : les boss d'endgame affrontés, un par arène : passages (arènes ouvertes),
    /// morts, temps moyen d'un passage et date du dernier.
    fn bosses_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(book) = self.coach.as_mut().and_then(|c| c.maps()) else { return };
        let mut bosses: Vec<(String, Vec<&mapping::Run>)> = Vec::new();
        for r in book.runs.iter().filter(|r| r.kind == mapping::RunKind::Boss) {
            match bosses.iter_mut().find(|(c, _)| *c == r.code) {
                Some((_, runs)) => runs.push(r),
                None => bosses.push((r.code.clone(), vec![r])),
            }
        }
        bosses.sort_by_key(|(_, runs)| std::cmp::Reverse(runs.last().map_or(0, |r| r.last)));
        let dim = Color32::GRAY;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 0 }).show(ui, |ui| {
                ui::section_header(ui, tr!("BOSS D'ENDGAME"), Some(bosses.len()), |_| {});
                if bosses.is_empty() {
                    ui.label(RichText::new(tr!("Les boss d'endgame s'afficheront ici dès ta première arène : tentatives, morts, temps passé.")).color(dim).size(15.0));
                    return;
                }
                let out = egui::ScrollArea::vertical().id_salt("bosses").auto_shrink([false, false]).show(ui, |ui| {
                    for (i, (code, runs)) in bosses.iter().enumerate() {
                        let last = runs[runs.len() - 1];
                        // Un passage = une arène ouverte (une clé) : les morts n'en relancent pas, le
                        // boss se reprend sur place.
                        let tries = runs.len() as u32;
                        let deaths: u32 = runs.iter().map(|r| r.deaths).sum();
                        let time: i64 = runs.iter().map(|r| r.active_secs()).sum();
                        if i > 0 {
                            ui.add_space(-ui.spacing().item_spacing.y);
                        }
                        let band = ui.painter().with_clip_rect(ui.clip_rect().expand2(Vec2::new(8.0, 0.0)));
                        let stripe = band.add(egui::Shape::Noop);
                        // Comme l'historique des maps : l'arène à gauche, puis en colonnes depuis la
                        // droite le temps moyen d'un passage, les morts, les passages et la date du
                        // dernier (la date puis les passages disparaissent quand la place manque).
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), egui::Sense::hover());
                        const GAP: f32 = 18.0;
                        let text_w = |t: &str, size: f32| ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(size), Color32::WHITE).size().x.ceil();
                        let dur_w = text_w("00 min 00 s", 18.0);
                        let count_w = 22.0 + 3.0 + text_w("00", 15.0);
                        let tries_w = text_w(&trn!(10, "{n} passage", "{n} passages"), 13.5);
                        let date_w = text_w("29 sept. 23:59", 13.5);
                        let room = |extra: f32| rect.width() - dur_w - count_w - 2.0 * GAP - extra >= 200.0;
                        let date = room(tries_w + date_w + 2.0 * GAP);
                        let show_tries = room(tries_w + GAP);
                        let mut right = rect.right();
                        let mut take = |w: f32| {
                            let r = egui::Rect::from_min_max(egui::pos2(right - w, rect.top()), egui::pos2(right, rect.bottom()));
                            right -= w + GAP;
                            r
                        };
                        let ltr = egui::Layout::left_to_right(egui::Align::Center);
                        let col = |ui: &mut egui::Ui, r: egui::Rect, layout: egui::Layout, add: &mut dyn FnMut(&mut egui::Ui)| {
                            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r).layout(layout), |ui| add(ui));
                        };
                        let icons = &mut self.passive_icons;
                        col(ui, take(dur_w), egui::Layout::right_to_left(egui::Align::Center), &mut |ui| {
                            ui.label(RichText::new(duration_text(time / tries as i64)).color(Color32::from_gray(225)).size(18.0).strong());
                        });
                        col(ui, take(count_w), ltr, &mut |ui| icon_count(ui, icons, TOMB_ICON, deaths, &trn!(deaths, "{n} mort", "{n} morts"), Color32::from_rgb(225, 90, 80)));
                        if show_tries {
                            col(ui, take(tries_w), ltr, &mut |ui| {
                                ui.label(RichText::new(trn!(tries, "{n} passage", "{n} passages")).color(dim).size(13.5));
                            });
                        }
                        if date {
                            col(ui, take(date_w), ltr, &mut |ui| {
                                ui.label(RichText::new(date_text(last.start)).color(dim).size(13.5));
                            });
                        }
                        let left = egui::Rect::from_min_max(rect.min, egui::pos2(right, rect.bottom()));
                        col(ui, left, ltr, &mut |ui| {
                            ui.allocate_ui_with_layout(Vec2::new(44.0, 52.0), ltr, |ui| {
                                tier_image(ui, icons, last, 38.0, dim);
                            });
                            ui.vertical(|ui| {
                                ui.add_space(7.0);
                                ui.add(egui::Label::new(RichText::new(run_name(last)).color(Color32::from_gray(220)).size(16.5)).truncate());
                                ui.add(egui::Label::new(RichText::new(boss_mechanic(code)).color(dim).size(13.5)).truncate());
                            });
                        });
                        let row = egui::InnerResponse::new((), ui.interact(rect, ui.id().with(("boss_line", i)), egui::Sense::hover()));
                        row.response.clone().on_hover_ui(|ui| {
                            ui.label(RichText::new(format!("{} · {}", run_name(last), boss_mechanic(code))).color(ui::GOLD));
                            ui.label(RichText::new(tr!("{tries} · {deaths} au total", tries = trn!(tries, "{n} passage", "{n} passages"), deaths = trn!(deaths, "{n} mort", "{n} morts"))).color(Color32::LIGHT_GRAY).size(13.5));
                            ui.label(RichText::new(tr!("{each} par passage en moyenne · {total} au total", each = duration_text(time / tries as i64), total = duration_text(time))).color(Color32::LIGHT_GRAY).size(13.5));
                            ui.label(RichText::new(tr!("dernier passage le {date}", date = date_text(last.start))).color(Color32::LIGHT_GRAY).size(13.5));
                        });
                        if i % 2 == 1 {
                            band.set(stripe, egui::Shape::rect_filled(row.response.rect.expand2(Vec2::new(8.0, 0.0)), 0.0, Color32::from_rgb(21, 20, 18)));
                        }
                    }
                });
                ui::fade_scroll(ui, &out, true);
            });
        });
    }

    fn market_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        const CATEGORIES: &[(&str, &str)] = MARKET_CATEGORIES;
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
        // Détail d'un objet ouvert depuis la liste : il la remplace, « Retour » y ramène.
        let opened = self.market_open.as_ref().and_then(|name| prices.data.lock().unwrap().as_ref()?.market.iter().find(|l| &l.name == name).cloned());
        if let Some(line) = opened {
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                let margin = egui::Margin { left: 0, right: 8, top: 0, bottom: 0 };
                let mut back = false;
                egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(22.0);
                        let r = ui.add(egui::Label::new(RichText::new(tr!("MARCHÉ")).color(ui::GOLD_DIM).size(13.0).strong().underline()).sense(egui::Sense::click()));
                        back |= r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
                        ui.label(RichText::new("›").color(ui::GOLD_DIM).size(14.0));
                        ui.label(RichText::new(tr!("DÉTAIL")).color(ui::GOLD).size(13.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            back |= ui::action_button(ui, tr!("⬅  Retour")).on_hover_text(tr!("Revenir à la liste")).clicked();
                        });
                    });
                    ui.add_space(2.0);
                    let line = ui.available_rect_before_wrap();
                    ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.6)));
                    ui.add_space(8.0);
                });
                let out = egui::ScrollArea::vertical().id_salt("market_detail").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::new().inner_margin(margin).show(ui, |ui| self.market_detail(ui, &line, None, &prices, &shown_league, tr!("COURS DU MARCHÉ")));
                });
                ui::fade_scroll(ui, &out, true);
                if back {
                    self.market_open = None;
                }
            });
            return;
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
                                && (needle.is_empty() || l.name.to_lowercase().contains(&needle) || gametext::name(&l.name).to_lowercase().contains(&needle))
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
        let mut league_pick: Option<String> = None;
        let mut open: Option<String> = None;
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
                    ui::section_header(ui, tr!("MARCHÉ"), None, |ui| {
                        if busy {
                            ui.add(egui::Spinner::new().size(16.0).color(ui::GOLD));
                        } else {
                            refresh = ui::action_button(ui, tr!("⟳  Actualiser"))
                                .on_hover_text(tr!("Recharger les cours depuis poe.ninja"))
                                .clicked();
                        }
                        if let Some(a) = age {
                            let text = if a < 90 {
                                tr!("à l'instant").to_string()
                            } else {
                                tr!("il y a {n} min", n = a / 60)
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
                                                    tr!("{league} (personnage)", league = l)
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
                                        .on_hover_text(tr!("Ligue dont les cours sont affichés"));
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
                            .map(|(_, l)| i18n::tr(l))
                            .unwrap_or(tr!("Monnaie"));
                        egui::ComboBox::from_id_salt("market_category")
                            .selected_text(RichText::new(shown).color(ui::GOLD))
                            .show_ui(ui, |ui| {
                                for (key, label) in CATEGORIES {
                                    ui.selectable_value(
                                        &mut self.market_category,
                                        key.to_string(),
                                        i18n::tr(label),
                                    );
                                }
                            });
                        ui.add(
                            // À la hauteur de la liste voisine.
                            egui::TextEdit::singleline(&mut self.market_search)
                                .hint_text(tr!("rechercher"))
                                .desired_width(ui.available_width())
                                .min_size(Vec2::new(0.0, ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y))
                                .vertical_align(egui::Align::Center),
                        );
                    });
                    ui.add_space(6.0);
                    if lines.is_empty() {
                        let text = if age.is_none() {
                            tr!("Chargement des cours…")
                        } else {
                            tr!("Aucun résultat")
                        };
                        ui.label(RichText::new(text).color(Color32::GRAY));
                        return;
                    }
                    let out = egui::ScrollArea::vertical()
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
                                            RichText::new(gametext::name(&l.name)).color(Color32::from_gray(225)),
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
                                                        "{sign}{}",
                                                        i18n::percent(format!("{:.0}", l.change))
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
                                // Clic : l'écran de détail de l'objet.
                                let click = ui.interact(row.response.rect, ui.id().with(("market_row", &l.id)), egui::Sense::click());
                                if click.hovered() {
                                    ui.painter().rect_filled(row.response.rect.expand2(Vec2::new(4.0, 2.0)), 3.0, Color32::from_white_alpha(6));
                                }
                                if click.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    open = Some(l.name.clone());
                                }
                            }
                        });
                    ui::fade_scroll(ui, &out, true);
                });
        });
        if open.is_some() {
            self.market_open = open;
        }
        if let Some(l) = league_pick {
            self.market_league = (l != self.league).then(|| {
                let p = prices::Prices::start(self.dirs.cache_dir().to_path_buf(), l.clone());
                (l, p)
            });
        }
    }

    /// Ligne du marché d'échange d'un objet diagnostiqué qui ne se porte pas (monnaie, rune,
    /// omen…) : il a alors l'écran de détail du marché à la place du diagnostic.
    fn market_line_for(&self, it: &Item) -> Option<prices::MarketLine> {
        if self.target_slot(it).is_some() || it.rarity == item::Rarity::Unique {
            return None;
        }
        let data = self.prices.data.lock().unwrap();
        data.as_ref()?.market.iter().find(|l| l.name == it.base || l.name == it.name).cloned()
    }

    /// Historique du cours d'une ligne du marché, chargé en arrière-plan à la première demande.
    fn market_history(&self, line: &prices::MarketLine, league: &str, ex_per_div: f64) -> Option<Result<Vec<(String, f64)>, String>> {
        let key = format!("{league}|{}", line.details_id);
        let mut map = self.market_history.lock().unwrap();
        if let Some(h) = map.get(&key) {
            return h.clone();
        }
        map.insert(key.clone(), None);
        let (shared, league, category, id) = (self.market_history.clone(), league.to_string(), line.category.clone(), line.details_id.clone());
        std::thread::spawn(move || {
            let got = prices::history(&league, &category, &id, ex_per_div).map_err(|e| e.to_string());
            shared.lock().unwrap().insert(key, Some(got));
        });
        None
    }

    /// Écran de détail d'un objet du marché : son en-tête et son texte (ceux de l'objet
    /// diagnostiqué, sinon d'un objet du même type dans l'historique), puis jusqu'en bas son cours
    /// en grand dans les trois devises, sa courbe, le volume échangé et les liens.
    fn market_detail(&mut self, ui: &mut egui::Ui, line: &prices::MarketLine, it: Option<&Item>, prices: &prices::Prices, league: &str, title: &str) {
        let (ex_per_div, chaos_ex) = {
            let data = prices.data.lock().unwrap();
            data.as_ref().map(|p| (p.exalted_per_divine, p.currency("Chaos Orb").map(|c| p.to_ex(c)).unwrap_or(0.0))).unwrap_or((0.0, 0.0))
        };
        let ex = line.value * ex_per_div;
        let unit = |v: f64| {
            if ex_per_div > 0.0 && v >= ex_per_div {
                (ex_per_div, "div")
            } else if chaos_ex > 1.0 && v >= chaos_ex {
                (chaos_ex, "chaos")
            } else {
                (1.0, "ex")
            }
        };
        let fmt = |v: f64| Self::fmt_in(v, unit(v));
        // Bande de titre, comme au diagnostic.
        let category = MARKET_CATEGORIES.iter().find(|(k, _)| *k == line.category).map(|(_, l)| i18n::tr(l)).unwrap_or(line.category.as_str());
        ui.add_space(4.0);
        ui.label(RichText::new(title).color(ui::GOLD).size(15.5).strong());
        ui.label(RichText::new(format!("{category} · {league}")).color(Color32::GRAY));
        ui.add_space(6.0);
        ui.separator();
        let known = it.cloned().or_else(|| self.history.iter().find(|h| h.base == line.name || h.name == line.name).cloned());
        match &known {
            Some(it) => self.item_preview(ui, it, None, None),
            None => {
                let tex = self.assets.texture_url(format!("{}{}", prices::IMAGE_HOST, line.image));
                ui::item_header(ui, "market", tex.as_ref(), &line.name, ui::rarity_color(item::Rarity::Currency), None, Vec::new(), None);
            }
        }
        ui.add_space(10.0);
        // Le cours, dans une carte qui va jusqu'en bas.
        let bottom = ui.clip_rect().bottom() - 8.0;
        let card_h = (bottom - ui.cursor().top()).max(320.0);
        let up = line.change >= 0.0;
        let trend = if up { ui::SOCKET_GOOD } else { ui::SOCKET_WRONG };
        let history = (!line.details_id.is_empty() && ex_per_div > 0.0).then(|| self.market_history(line, league, ex_per_div)).flatten();
        let mut period = self.market_period;
        egui::Frame::new()
            .fill(ui::CARD_FILL)
            .stroke(egui::Stroke::new(1.0_f32, ui::GOLD_DIM.gamma_multiply(0.55)))
            .corner_radius(6)
            .inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let top = ui.cursor().top();
                // Prix en grand dans les trois devises, la plus parlante en or.
                ui.horizontal(|ui| {
                    let main = unit(ex).1;
                    let mut parts = vec![(1.0, "ex")];
                    if chaos_ex > 0.0 {
                        parts.push((chaos_ex, "chaos"));
                    }
                    if ex_per_div > 0.0 {
                        parts.push((ex_per_div, "div"));
                    }
                    for (i, (per, name)) in parts.into_iter().enumerate() {
                        if i > 0 {
                            ui.add_space(18.0);
                        }
                        let v = ex / per;
                        let n = if v >= 100.0 {
                            format!("{v:.0}")
                        } else if v >= 1.0 {
                            i18n::decimal(format!("{v:.1}").trim_end_matches(".0").to_string())
                        } else if v >= 0.001 {
                            i18n::decimal(format!("{v:.3}").trim_end_matches('0').to_string())
                        } else {
                            i18n::decimal("< 0.001".into())
                        };
                        let (size, color) = if name == main { (30.0, Color32::from_rgb(235, 200, 60)) } else { (22.0, Color32::from_rgb(225, 220, 210)) };
                        ui.label(RichText::new(n).size(size).color(color).strong());
                        ui.label(RichText::new(name).size(size * 0.6).color(Color32::GRAY));
                    }
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}{}", if up { "+" } else { "" }, i18n::percent(format!("{:.0}", line.change)))).color(trend).strong());
                    ui.label(RichText::new(tr!("sur 7 jours")).color(Color32::GRAY));
                });
                ui.add_space(8.0);
                // Titre de la courbe et choix de la période.
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr!("ÉVOLUTION DU COURS")).color(ui::GOLD).size(13.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for (i, label) in [tr!("Tout"), tr!("30 j"), tr!("7 j")].iter().enumerate().map(|(i, l)| (2 - i, l)) {
                            let on = period == i;
                            let text = RichText::new(*label).size(12.5).color(if on { ui::GOLD } else { Color32::GRAY });
                            let r = ui.add(egui::Label::new(if on { text.strong().underline() } else { text }).sense(egui::Sense::click()));
                            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                period = i;
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                // La courbe prend la place laissée par le pied (mesuré à l'image précédente).
                let foot_id = ui.id().with("market_foot");
                let foot_h: f32 = ui.data(|d| d.get_temp(foot_id)).unwrap_or(70.0);
                let used = ui.cursor().top() - top;
                let chart_h = (card_h - 20.0 - used - foot_h - 12.0).max(140.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), chart_h), egui::Sense::hover());
                match &history {
                    Some(Ok(points)) => {
                        // Repères dans la devise du prix, avec deux décimales au besoin.
                        let (per, name) = unit(ex);
                        let axis = |v: f64| {
                            let v = v / per;
                            let n = if v >= 100.0 { format!("{v:.0}") } else if v >= 10.0 { format!("{v:.1}") } else { format!("{v:.2}") };
                            format!("{} {name}", i18n::decimal(n))
                        };
                        let n = [7, 30, usize::MAX][period].min(points.len());
                        ui::price_chart(ui, rect, &points[points.len() - n..], trend, &axis);
                    }
                    Some(Err(e)) => {
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tr!("Historique indisponible : {e}", e), egui::FontId::proportional(13.0), Color32::GRAY);
                    }
                    None => {
                        ui.put(egui::Rect::from_center_size(rect.center(), Vec2::splat(24.0)), egui::Spinner::new().size(22.0).color(ui::GOLD));
                        ui.ctx().request_repaint_after(Duration::from_millis(250));
                    }
                }
                ui.add_space(12.0);
                let foot_top = ui.cursor().top();
                // Volume : poe.ninja ne compte pas d'annonces sur le marché d'échange, mais ce qui s'échange.
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr!("Volume échangé :")).color(Color32::from_gray(225)).strong());
                    let units = if line.value > 0.0 { tr!(" (≈ {n} unités)", n = Self::fmt_count(line.volume / line.value)) } else { String::new() };
                    ui.label(RichText::new(tr!("{amount} par jour{units}", amount = fmt(line.volume * ex_per_div), units)).color(Color32::LIGHT_GRAY));
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    let have = if line.id == "divine" { "exalted" } else if line.value >= 1.0 || line.id == "exalted" { "divine" } else { "exalted" };
                    ui.hyperlink_to(tr!("Acheter ou vendre sur le trade ↗"), uniques::exchange_url(league, &line.id, have));
                    if let Some(url) = prices.data.lock().unwrap().as_ref().and_then(|p| p.page_url(false, &line.name, &line.name)) {
                        ui.add_space(12.0);
                        ui.hyperlink_to(tr!("Voir sur poe.ninja ↗"), url);
                    }
                });
                let h = ui.cursor().top() - foot_top;
                if (h - foot_h).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(foot_id, h));
                    ui.ctx().request_repaint();
                }
            });
        self.market_period = period;
    }

    /// « 11 100 » : un nombre entier, les milliers séparés.
    fn fmt_count(n: f64) -> String {
        let digits = format!("{}", n.round().max(0.0) as u64);
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(if i18n::english() { ',' } else { '\u{202f}' });
            }
            out.push(c);
        }
        out
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
                            Some(left) => tr!("{step} (limite de l'API : encore {n} s)", step, n = left.as_secs() + 1),
                            None => step,
                        };
                        ui.add(egui::Spinner::new().size(16.0).color(ui::GOLD)).on_hover_text(step);
                        ui.ctx().request_repaint_after(Duration::from_millis(250));
                        ui.add_space(2.0);
                    } else if done && self.stored.trade_prices {
                        again = ui::action_button(ui, "⟳").on_hover_text(tr!("Réestimer : relancer la recherche sur le site de trade (les prix bougent)")).clicked();
                        ui.add_space(2.0);
                    } else if ninja {
                        if self.prices.busy() {
                            ui.add(egui::Spinner::new().size(16.0).color(ui::GOLD));
                            ui.ctx().request_repaint_after(Duration::from_millis(250));
                        } else {
                            again = ui::action_button(ui, "⟳").on_hover_text(tr!("Recharger les cours de poe.ninja")).clicked();
                        }
                        ui.add_space(2.0);
                    }
                    ui.label(RichText::new(tr!("ESTIMATION")).color(ui::GOLD).size(13.0).strong());
                    // Jamais estimé (ou échec) : le bouton pour lancer l'estimation, à la place de
                    // « Détails ».
                    if self.needs_estimate(it) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (label, hint) = match est {
                                Some(trade::Estimate::Failed(_)) => (tr!("⟳  Réessayer"), tr!("Relancer l'estimation sur le site de trade")),
                                _ => (tr!("💰  Estimer"), tr!("Cherche des annonces comparables sur le site de trade")),
                            };
                            estimate = ui::action_button(ui, label).on_hover_text(hint).clicked();
                        });
                    }
                    if done || ninja {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let hint = if done { tr!("Comment le prix a été trouvé : recherche, critères, annonces") } else { tr!("D'où vient le prix : cours de poe.ninja") };
                            open = ui::action_button(ui, tr!("🔍  Détails")).on_hover_text(hint).clicked();
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
            ui.label(RichText::new(tr!("Pas d'estimation pour cet objet.")).color(Color32::GRAY));
            return;
        }
        ui.add_space(4.0);
        self.item_preview(ui, it, None, None);
        // Le prix : les mêmes lignes qu'au diagnostic.
        self.price_title(ui, it, true);
        self.value_rows(ui, it, &None);
        // La source du prix : sa page sur poe.ninja (sinon, pour un unique, ses annonces).
        let page = self.prices.data.lock().unwrap().as_ref().and_then(|p| p.page_url(it.rarity == item::Rarity::Unique, &it.name, &it.base));
        ui.add_space(10.0);
        ui::zone_title(ui, tr!("SOURCE"), "");
        match page {
            Some(url) => {
                ui.hyperlink_to(tr!("Voir sur poe.ninja"), url);
            }
            None if it.rarity == item::Rarity::Unique => {
                ui.hyperlink_to(tr!("Voir les annonces sur le site de trade"), uniques::trade_url(&self.league, &it.name, Some(&it.base)));
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
                ui.label(RichText::new(tr!("{label} :", label)).color(Color32::from_gray(225)).strong());
                add(ui);
            };
            if ui.is_sizing_pass() {
                ui.horizontal(|ui| inner(ui));
            } else {
                ui.horizontal_wrapped(|ui| inner(ui));
            }
        };
        let advice = |ui: &mut egui::Ui| {
            row(ui, tr!("Conseil"), &mut |ui| {
                ui.label(RichText::new(value.label.advice()).color(ui::value_color(value.label)).strong());
            });
        };
        // En attente d'estimation : les lignes restent, avec « — » en gris.
        let unknown = |ui: &mut egui::Ui| {
            for label in [tr!("Confiance"), tr!("Conseil"), tr!("Basé sur")] {
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
                        let span = tr!("de {low} à {high}", low = Self::fmt_in(low, unit), high = Self::fmt_in(high, unit));
                        let base = if *base_only { tr!("base ") } else { "" };
                        let p = e.confidence_pct();
                        row(ui, tr!("Prix estimé"), &mut |ui| {
                            // Confiance faible : la fourchette d'abord, le prix n'est qu'un repère.
                            if p < 50 && high - low >= 0.1 {
                                ui.label(RichText::new(format!("{base}{span}")).color(gold).strong());
                                ui.label(RichText::new(tr!("(repère {price})", price = self.fmt_price(estimate))).color(Color32::GRAY));
                            } else {
                                ui.label(RichText::new(format!("{base}≈ {}", self.fmt_price(estimate))).color(gold).strong());
                                let range = if high - low >= 0.1 { format!(" · {span}") } else { String::new() };
                                ui.label(RichText::new(format!("({}){range}", self.price_all(estimate))).color(Color32::GRAY));
                            }
                        });
                        row(ui, tr!("Confiance"), &mut |ui| {
                            ui.label(RichText::new(format!("{} ({})", i18n::percent(p), e.confidence())).color(ui::confidence_color(p)));
                        });
                    }
                    _ if *total == 0 => row(ui, tr!("Prix estimé"), &mut |ui| {
                        ui.label(RichText::new(tr!("aucune annonce comparable en achat immédiat")).color(Color32::LIGHT_GRAY));
                    }),
                    _ => {
                        let min = prices_ex.iter().copied().fold(f64::INFINITY, f64::min);
                        let from = if min.is_finite() { tr!(", dès {price}", price = self.fmt_price(min)) } else { String::new() };
                        let text = tr!("trop peu d'annonces pour estimer ({total}{from} ; il en faut {min})", total, from, min = trade::MIN_PRICED);
                        row(ui, tr!("Prix estimé"), &mut |ui| {
                            ui.label(RichText::new(&text).color(Color32::LIGHT_GRAY));
                        });
                    }
                }
                advice(ui);
                // Sur quoi repose l'estimation (annonces proches sur celles lues, éparpillement).
                let kept = listings.iter().filter(|l| l.kept).count();
                let mut basis = if *score == 0.0 || listings.is_empty() {
                    trn!(*total, "{n} annonce en achat immédiat", "{n} annonces en achat immédiat")
                } else {
                    trn!(kept, "{n} proche sur {read} annonces lues", "{n} proches sur {read} annonces lues", read = listings.len())
                };
                if let Some(spread) = e.spread().filter(|s| *s >= 2.0) {
                    basis.push_str(&tr!(" · prix éparpillés ×{n}", n = spread.round()));
                }
                row(ui, tr!("Basé sur"), &mut |ui| {
                    ui.label(RichText::new(&basis).color(Color32::GRAY));
                });

            }
            Some(trade::Estimate::Pending(msg)) => {
                row(ui, tr!("Prix estimé"), &mut |ui| {
                    ui.add(egui::Spinner::new().color(ui::GOLD));
                    match trade::wait_left() {
                        Some(left) => {
                            ui.label(RichText::new(tr!("limite de l'API de trade : encore {n} s", n = left.as_secs() + 1)).color(Color32::LIGHT_GRAY));
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
                row(ui, tr!("Prix estimé"), &mut |ui| {
                    ui.label(RichText::new(e).color(Color32::from_rgb(230, 120, 100)));
                });
                unknown(ui);
            }
            None if trade::priceable(it) => {
                row(ui, tr!("Prix estimé"), &mut |ui| {
                    if self.stored.trade_prices {
                        ui.label(RichText::new("—").color(Color32::from_gray(110)));
                    } else {
                        ui.label(RichText::new(tr!("estimation sur le trade coupée dans les réglages")).color(Color32::GRAY));
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
                    row(ui, tr!("Prix estimé"), &mut |ui| {
                        ui.label(RichText::new(self.fmt_price(ex)).color(gold).strong());
                        ui.label(RichText::new(format!("({})", self.price_all(ex))).color(Color32::GRAY));
                    });
                    advice(ui);
                    let basis = match listings {
                        Some(n) => trn!(n, "{n} annonce, cours de poe.ninja", "{n} annonces, cours de poe.ninja"),
                        None => tr!("cours de poe.ninja").to_string(),
                    };
                    row(ui, tr!("Basé sur"), &mut |ui| {
                        ui.label(RichText::new(&basis).color(Color32::GRAY));
                    });
                    // Le résumé redit le prix : il est dans le détail.
                    return;
                }
                None => {
                    row(ui, tr!("Prix"), &mut |ui| {
                        ui.label(RichText::new(tr!("inconnu")).color(Color32::GRAY));
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
                let hint = if ninja { tr!("Recharger les cours de poe.ninja") } else { tr!("Réestimer : relancer la recherche sur le site de trade (les prix bougent)") };
                again = ui::action_button(ui, "⟳").on_hover_text(hint).clicked();
            }
            ui.label(RichText::new(tr!("PRIX")).color(ui::GOLD_DIM).small().strong());
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
            Some(trade::Estimate::Done { listings, total, .. }) => tr!("{read} sur {total} en vente", read = listings.len(), total),
            _ => String::new(),
        };
        // Même titre que les cartes (« ACTIVITÉ ») : sous-titre gris, trait dessous.
        ui::section_header_sub(ui, tr!("ANNONCES LUES"), &detail);
    }

    /// `listings_only` : la colonne de droite, les annonces lues seules (sans leur titre) ; sinon
    /// tout le reste.
    fn estimation_detail(&mut self, ui: &mut egui::Ui, it: &Item, listings_only: bool) {
        let Some(e) = self.trade.get(it).filter(|_| trade::priceable(it)) else {
            if listings_only {
                ui.label(RichText::new(tr!("Aucune : le prix vient des cours de poe.ninja.")).color(Color32::GRAY));
            } else {
                self.ninja_detail(ui, it);
            }
            return;
        };
        let trade::Estimate::Done { url, criteria, base_only, widened, listings, score, .. } = &e else { return };
        let (base_only, widened, score) = (*base_only, *widened, *score);
        if listings_only {
            if listings.is_empty() {
                ui.label(RichText::new(tr!("Aucune annonce gardée : réestime l'objet pour les voir.")).color(Color32::GRAY));
            } else {
                self.listing_cards(ui, it, listings, score, e.shares());
            }
            return;
        }
        let text = |ui: &mut egui::Ui, t: &str| {
            ui.label(RichText::new(t).color(Color32::GRAY));
        };
        ui.add_space(4.0);
        // La pièce entière, comme au diagnostic (lignes d'arme et DPS, implicites, mods).
        let view = self.target_slot(it).map(|s| {
            let card = self.build_card(Some(it), s);
            self.worn_view(s, it, card.as_ref())
        });
        self.item_preview(ui, it, None, view.as_ref());
        // Le prix : les mêmes lignes qu'au diagnostic.
        self.price_title(ui, it, false);
        self.value_rows(ui, it, &Some(e.clone()));
        ui.add_space(10.0);

        ui::zone_title(ui, tr!("RECHERCHE"), "");
        if base_only {
            text(ui, tr!("aucun mod reconnu : prix de la base, pour le craft"));
        }
        if widened {
            text(ui, &tr!("trop peu d'annonces de note proche (±{gap} %) : les plus proches ont été prises", gap = (trade::SCORE_GAP * 100.0) as u32));
        }
        // Mods cherchés ; ceux lâchés par la recherche élargie, en gris. Le nombre de mods exigé
        // n'est pas répété.
        for c in criteria {
            if c.starts_with("au moins ") && c.ends_with("comme cet objet") {
                continue;
            }
            match trade::criterion_text(c) {
                (text, true) => ui.label(RichText::new(format!("• {text}")).color(Color32::from_gray(110))).on_hover_text(tr!("Lâché par la recherche élargie : sa place reste exigée")),
                (text, false) => ui.label(RichText::new(format!("• {text}")).color(Color32::LIGHT_GRAY)),
            };
        }
        // La source du prix, comme pour un prix poe.ninja.
        ui.add_space(10.0);
        ui::zone_title(ui, tr!("SOURCE"), "");
        ui.hyperlink_to(tr!("Voir la recherche sur le site de trade"), url);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Alt maintenu : tout dans l'autre langue (interface et textes du jeu), le temps de l'appui.
        // Développement : `POE2_TRACKER_ALT=1` fait comme si Alt était maintenu.
        if i18n::set_flip(ctx.input(|i| i.modifiers.alt) || std::env::var_os("POE2_TRACKER_ALT").is_some()) {
            self.socket_tips.clear();
        }
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
            let runs = |c: &mut coach::Coach| c.maps().map_or(0, |b| b.runs.len());
            let before = runs(c);
            while let Ok(ev) = self.log_rx.try_recv() {
                for line in c.handle(&ev) {
                    self.feed.push_front(line);
                    changed = true;
                }
                changed |= matches!(ev, logwatch::Event::Area { .. } | logwatch::Event::Death { .. });
            }
            // Entrée dans une nouvelle map : le bloc du bas passe sur les maps.
            if runs(c) > before {
                self.bottom_maps = Some(true);
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

        // Emplacement de charme donné par la campagne : compte dans les charmes ouverts.
        self.gear.quest_charm_slots = self.quest_bonuses().charm_slots;
        // Plein écran (Talents, Atlas) : la vue seule, sans barre ni panneaux.
        let full = self.full_view && matches!(self.tab, views::Tab::Passives | views::Tab::Atlas);
        if full && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.full_view = false;
        }
        if full && self.full_view {
            self.sync_league(ctx);
            self.settings_window(ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(Color32::from_rgb(12, 11, 10)).inner_margin(12))
                .show(ctx, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    if self.tab == views::Tab::Atlas {
                        self.atlas_view(ui, rect);
                    } else {
                        self.build_view(ui, rect);
                    }
                    self.full_view_button(ui, rect);
                });
            return;
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
                        // Raccourcis, touches dessinées comme sur un clavier : ceux du jeu, puis
                        // ceux de l'app, chaque groupe sous son petit titre.
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let group = |ui: &mut egui::Ui, title: &str| {
                            ui.label(RichText::new(title).color(ui::GOLD_DIM).small().strong());
                            ui.add_space(4.0);
                        };
                        group(ui, tr!("EN JEU"));
                        ui::keys(ui, &["Ctrl", "C"]);
                        ui.label(RichText::new(tr!("diagnostiquer")).color(Color32::GRAY));
                        ui.add_space(14.0);
                        ui::keys(ui, &["Ctrl", "Alt", "C"]);
                        ui.label(RichText::new(tr!("suivre la pièce")).color(Color32::GRAY));
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                        group(ui, tr!("DANS L'APP"));
                        ui::keys(ui, &["Alt"]);
                        let other = if i18n::base() == i18n::Lang::Fr { i18n::Lang::En } else { i18n::Lang::Fr };
                        ui.label(RichText::new(tr!("maintenu : {lang}", lang = other.name())).color(Color32::GRAY));
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
        self.route_pick_window(ctx);
        // La grille garde ses proportions et prend toute la place possible ; le diagnostic
        // (à droite) et le fil (en bas) absorbent le reste, sans bande vide autour du fond.
        let free = ctx.available_rect();
        let scale = ui::canvas_scale(free.width() - SIDE_MIN, free.height() - FEED_MIN - TAB_H, self.bar_h);
        let side_w = (free.width() - ui::CANVAS_W * scale).max(SIDE_MIN);
        let feed_h = (free.height() - ui::CANVAS_H * scale - ui::top_pad(scale, self.bar_h) - TAB_H).max(FEED_MIN);
        egui::TopBottomPanel::bottom("feed").resizable(false).exact_height(feed_h).show(ctx, |ui| self.feed_panel(ui));
        let comparison = self.diagnosed.as_ref().and_then(|it| self.compare(it));
        let mut glow: HashMap<Slot, Color32> = comparison
            .iter()
            .flat_map(|c| c.against.iter().map(|s| (*s, ui::verdict_style(c.verdict).1)))
            .collect();
        // Anneau, charme : chaque emplacement à la couleur de sa réponse.
        if let Some(b) = self.diag_bands.as_ref().filter(|b| comparison.is_some() && b.slots.len() > 1) {
            glow = b.slots.iter().map(|d| (d.slot, d.answer.1)).collect();
        }
        egui::SidePanel::right("diagnostic").resizable(false).exact_width(side_w).show(ctx, |ui| {
            // Onglets en bas, sur la même ligne que ceux de gauche ; le contenu au-dessus.
            let side = ui.available_rect_before_wrap();
            let full = egui::Rect::from_min_max(side.min, egui::pos2(side.max.x, side.max.y - TAB_H));
            let tabs_rect = egui::Rect::from_min_max(egui::pos2(side.min.x, full.max.y), side.max);
            // L'onglet Boss, expérimental, seulement s'il est activé dans les réglages.
            if !self.stored.boss_tab && self.side_tab == views::SideTab::Bosses {
                self.side_tab = views::SideTab::Maps;
            }
            let tabs: Vec<_> = views::SideTab::ALL.iter().filter(|t| self.stored.boss_tab || **t != views::SideTab::Bosses).map(|t| (*t, t.label())).collect();
            if let Some(tab) = ui::tab_strip(ui, tabs_rect, &tabs, self.side_tab) {
                self.side_tab = tab;
            }
            if self.side_tab == views::SideTab::Market {
                self.market_panel(ui, full);
                return;
            }
            if self.side_tab == views::SideTab::Maps {
                self.maps_panel(ui, full);
                return;
            }
            if self.side_tab == views::SideTab::Bosses {
                self.bosses_panel(ui, full);
                return;
            }
            // Trois niveaux : historique, diagnostic, détail de l'estimation. Au diagnostic, valeur
            // et estimation sont collées en bas, toujours visibles ; le reste défile au-dessus. La
            // colonne est découpée à la main : la hauteur du bas, mesurée à l'affichage précédent,
            // est retirée à la zone qui défile.
            let diagnosed = self.diagnosed.clone();
            let detail = diagnosed.as_ref().is_some_and(|d| self.estimate_open.as_deref() == Some(d.raw.as_str()));
            let value_h_id = ui.id().with("value_h");
            // Objet qui ne se porte pas, coté sur le marché d'échange : l'écran de détail du marché.
            let market_line = diagnosed.as_ref().filter(|_| !detail).and_then(|it| self.market_line_for(it));
            let value_h: f32 = if diagnosed.is_some() && !detail && market_line.is_none() { ui.data(|d| d.get_temp(value_h_id)).unwrap_or(160.0) } else { 0.0 };
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
                            let crumbs = [tr!("HISTORIQUE"), tr!("DIAGNOSTIC"), tr!("ESTIMATION")];
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
                                    let hint = if depth == 2 { tr!("Revenir au diagnostic") } else { tr!("Revenir à l'historique") };
                                    back = ui::action_button(ui, tr!("⬅  Retour")).on_hover_text(hint).clicked();
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
                        (Some(it), _) if market_line.is_some() => egui::ScrollArea::vertical().id_salt("diagnostic_scroll").auto_shrink([false, false]).show(ui, |ui| {
                            let (prices, league) = (self.prices.clone(), self.league.clone());
                            egui::Frame::new().inner_margin(margin).show(ui, |ui| self.market_detail(ui, market_line.as_ref().unwrap(), Some(it), &prices, &league, tr!("OBJET DIAGNOSTIQUÉ")));
                            None
                        }),
                        _ => egui::ScrollArea::vertical().id_salt("diagnostic_scroll").auto_shrink([false, false]).show(ui, |ui| {
                            egui::Frame::new().inner_margin(margin).show(ui, |ui| self.diagnostic(ui, comparison.as_ref())).inner
                        }),
                    }
                })
                .inner;
            ui::fade_scroll(ui, &scroll, true);
            // Verdict du diagnostic, et dessous le prix comme dans l'historique (loader le temps de
            // l'estimation) : à leur place dans l'en-tête, mais fixes, par-dessus ce qui défile.
            if let Some(it) = diagnosed.as_ref().filter(|_| !detail && market_line.is_none()) {
                let anchor = egui::Rect::from_min_max(scroll.inner_rect.min + Vec2::new(0.0, 4.0), egui::pos2(scroll.inner_rect.max.x - 8.0, scroll.inner_rect.min.y + 90.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(anchor).layout(egui::Layout::top_down(egui::Align::Max)), |ui| {
                    if let Some((text, color)) = scroll.inner {
                        ui::badge(ui, &text, color);
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
            if let Some(it) = diagnosed.filter(|_| !detail && market_line.is_none()) {
                let bottom = egui::Rect::from_min_max(egui::pos2(full.min.x, split), full.max);
                let shown = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bottom), |ui| {
                    egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 8 }).show(ui, |ui| {
                        // Comparer à l'équipé, comparer au build : les modales de comparaison.
                        if let Some(b) = self.diag_bands.clone() {
                            let mut open = None;
                            // Face à ta pièce : de combien cette pièce rapproche (ou éloigne) du build.
                            let gap = |d: &SlotDiag| {
                                d.gap.map(|g| {
                                    let color = if g > 0 { ui::SOCKET_GOOD } else if g < 0 { ui::SOCKET_WRONG } else { Color32::GRAY };
                                    (format!("{}{}", if g > 0 { "+" } else if g < 0 { "−" } else { "" }, i18n::percent(g.abs())), color)
                                })
                            };
                            let build = |d: &SlotDiag| d.it_score.filter(|_| d.build).map(|s| (i18n::percent(s), ui::score_color(s)));
                            match b.slots.as_slice() {
                                [d] => {
                                    if ui::click_band(ui, tr!("Comparer à l'équipé"), gap(d).map(|(v, c)| (tr!("vers le build"), v, c)), d.worn, true).clicked() {
                                        open = Some((d.slot, DetailMode::DiagWorn));
                                    }
                                    ui.add_space(4.0);
                                    if ui::click_band(ui, tr!("Comparer au build"), build(d).map(|(v, c)| (tr!("cette pièce"), v, c)), d.build, true).clicked() {
                                        open = Some((d.slot, DetailMode::DiagBuild));
                                    }
                                }
                                // Anneau, charme : une puce par emplacement, chacune ouvre sa modale.
                                slots => {
                                    let chips = |f: &dyn Fn(&SlotDiag) -> (Option<(String, Color32)>, bool)| -> Vec<ui::Chip> {
                                        slots.iter().map(|d| {
                                            let (value, enabled) = f(d);
                                            ui::Chip { label: d.slot.side().unwrap_or("").to_string(), value, enabled }
                                        }).collect()
                                    };
                                    if let Some(i) = ui::chip_band(ui, tr!("Comparer à l'équipé"), &chips(&|d| (gap(d), d.worn))) {
                                        open = Some((slots[i].slot, DetailMode::DiagWorn));
                                    }
                                    ui.add_space(4.0);
                                    if let Some(i) = ui::chip_band(ui, tr!("Comparer au build"), &chips(&|d| (build(d), d.build))) {
                                        open = Some((slots[i].slot, DetailMode::DiagBuild));
                                    }
                                }
                            }
                            if let Some((slot, mode)) = open {
                                self.slot_detail = Some(slot);
                                self.detail_mode = mode;
                            }
                            ui.add_space(6.0);
                        }
                        self.value_panel(ui, &it)
                    });
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
        // Comparaisons ouvertes depuis le diagnostic : par-dessus toute la fenêtre, en plus grand.
        if self.slot_detail.is_some() && self.detail_mode != DetailMode::Worn {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("diag_detail")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
                ui.set_clip_rect(screen);
                self.slot_detail(ui, screen, 1.4, 1.4);
            });
        }
    }
}

/// Fiches au survol des pastilles du texte d'un build (gemme, pièce, passif).
impl richtext::Resolver for App {
    fn icon(&mut self, url: &str) -> Option<egui::TextureHandle> {
        self.assets.texture_url(url.to_string())
    }

    fn tip(&mut self, ui: &mut egui::Ui, span: &richtext::Span) {
        let group = span.group.as_deref().unwrap_or_default();
        let name = span.text.as_str();
        let icon = span.icon.as_deref().and_then(|u| self.assets.texture_url(u.to_string()));
        if group.to_lowercase().contains("gems") {
            let infos = self.gem_infos.get();
            egui::Frame::new().inner_margin(8).show(ui, |ui| views::gem_tooltip(ui, name, "", infos.as_ref().and_then(|m| m.get(name))));
            return;
        }
        if group == "passiveSkills" {
            let passive = self.coach.as_ref().and_then(|c| c.catalog().passives.values().find(|p| p.name == name).cloned());
            let stats = passive.map(|p| p.stats).unwrap_or_default();
            ui::node_tip(ui, icon.as_ref(), name, ui::GOLD, None, &stats, None);
            return;
        }
        // Pièce : l'unique avec ses mods, sinon le type de base (défenses, implicites, niveau).
        let unique = self.coach.as_ref().is_some_and(|c| c.catalog().uniques.contains_key(name));
        let mut view = if unique { self.unique_view(name) } else { self.base_view(name) };
        if view.image.is_none() {
            view.image = icon;
        }
        egui::Frame::new().inner_margin(8).show(ui, |ui| {
            ui.set_width(ui::worn_width(ui, &view).clamp(260.0, 520.0));
            ui::worn_column(ui, &view);
        });
    }
}

/// Catégories poe.ninja du marché d'échange et leur nom affiché.
const MARKET_CATEGORIES: &[(&str, &str)] = &[
    ("Currency", tkey!("Monnaie")),
    ("Fragments", tkey!("Fragments")),
    ("Runes", tkey!("Runes")),
    ("Essences", tkey!("Essences")),
    ("SoulCores", tkey!("Noyaux d'âme")),
    ("Idols", tkey!("Idoles")),
    ("Expedition", tkey!("Expédition")),
    ("Ritual", tkey!("Rituel")),
    ("Delirium", tkey!("Délire")),
    ("Breach", tkey!("Brèche")),
    ("Abyss", tkey!("Abîme")),
    ("UncutGems", tkey!("Gemmes brutes")),
    ("LineageSupportGems", tkey!("Gemmes de lignée")),
];

static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((\d+(?:\.\d+)?)-(\d+(?:\.\d+)?)\)").unwrap());

/// Mods d'un unique (texte PoB, plages « (10-20) ») face à ceux d'une pièce : pour chacun, sa
/// part atteinte (1 et plus : au moins le minimum de la plage ; 0 : absent). Un mod sans valeur
/// compte s'il est présent.
fn unique_matches(it: &Item, mods: &[String]) -> Vec<f64> {
    let have: Vec<(String, Option<f64>)> = it.mods.iter().map(|m| trade::template(m)).collect();
    mods.iter()
        .map(|m| {
            // La plage devient son minimum : c'est ce qu'il faut au moins avoir.
            let (tpl, want) = trade::template(&RANGE.replace_all(m, "$1"));
            match have.iter().find(|(t, _)| *t == tpl) {
                None => 0.0,
                Some((_, h)) => match want.filter(|w| *w > 0.0) {
                    Some(w) => (h.unwrap_or(0.0) / w).clamp(0.0, 2.0),
                    None => 1.0,
                },
            }
        })
        .collect()
}

/// Marque d'une part atteinte : ✓ au moins visée, ~ en dessous, ✗ absente.
fn match_mark(part: f64) -> (&'static str, Color32) {
    if part >= 1.0 {
        ("✓", ui::SOCKET_GOOD)
    } else if part > 0.0 {
        ("~", ui::EMPTY_SOCKET)
    } else {
        ("✗", ui::SOCKET_WRONG)
    }
}

/// De quoi afficher les bandeaux « comparer » du diagnostic : un emplacement, ou chacun de
/// ceux d'un anneau ou d'un charme.
#[derive(Clone)]
struct DiagBands {
    slots: Vec<SlotDiag>,
}

/// La pièce diagnostiquée face à un emplacement : pièce portée et recommandation présentes,
/// scores, écart, réponse.
#[derive(Clone)]
struct SlotDiag {
    slot: Slot,
    worn: bool,
    build: bool,
    it_score: Option<u32>,
    gap: Option<i64>,
    answer: (&'static str, Color32),
}

/// Contenu de la modale de comparaison.
#[derive(Clone, Copy, PartialEq)]
enum DetailMode {
    /// Clic sur une case : la pièce portée face à ce que recommande le build.
    Worn,
    /// Diagnostic : la pièce diagnostiquée face à ce que recommande le build.
    DiagBuild,
    /// Diagnostic : la pièce diagnostiquée face à la pièce portée.
    DiagWorn,
}

/// Un bloc de comparaison (ta pièce, ou celle du build), face à la pièce diagnostiquée : en-tête
/// de la pièce, lignes marquées, runes, score.
#[derive(Default)]
struct DiagSide {
    image: Option<egui::TextureHandle>,
    name: String,
    name_color: Color32,
    after: Option<String>,
    /// Lignes d'arme (dégâts, critique, attaques par seconde), colorées comme en jeu.
    weapon: Vec<Vec<(String, Color32)>>,
    props: Vec<ui::Parts>,
    prop_marks: Vec<Option<(&'static str, Color32)>>,
    implicits: Vec<String>,
    implicit_marks: Vec<Option<(&'static str, Color32)>>,
    /// Ligne : marque, couleur, texte.
    lines: Vec<(Option<(&'static str, Color32)>, Color32, String)>,
    notes: Vec<String>,
    runes: Vec<ui::RuneLine>,
    score: Option<u32>,
}

static WEAPON_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Physical|Elemental|Fire|Cold|Lightning|Chaos) Damage: (.+)$").unwrap());
static RANGE_PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)-(\d+)( \(augmented\))?").unwrap());
static ADDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Adds (\d+) to (\d+) (Fire|Cold|Lightning|Chaos|Physical) Damage").unwrap());
static RATE_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Critical Hit Chance|Attacks per Second|Reload Time): (.+?)( \(augmented\))?$").unwrap());

/// Couleur d'un type de dégâts, comme en jeu.
fn damage_color(kind: &str) -> Color32 {
    match kind {
        "Fire" => Color32::from_rgb(220, 90, 60),
        "Cold" => Color32::from_rgb(100, 150, 240),
        "Lightning" => Color32::from_rgb(240, 210, 60),
        "Chaos" => Color32::from_rgb(210, 70, 170),
        _ => Color32::from_gray(225),
    }
}

/// Lignes d'une arme comme en jeu : dégâts physiques, dégâts élémentaires (chaque fourchette dans
/// la couleur de son élément, retrouvé d'après les mods « Adds X to Y … Damage »), chaos, coups
/// critiques, attaques par seconde ; valeurs changées par les mods en bleu.
fn weapon_lines(it: &Item) -> Vec<Vec<(String, Color32)>> {
    let (label, aug) = (Color32::GRAY, Color32::from_rgb(136, 136, 255));
    let adds: Vec<(String, String, String)> = it.mods.iter().filter_map(|m| ADDS.captures(m).map(|c| (c[1].to_string(), c[2].to_string(), c[3].to_string()))).collect();
    // Dégâts par seconde entre parenthèses au bout des lignes : physiques, élémentaires, et le
    // total après les attaques par seconde.
    // Chaque ligne de dégâts donne le sien (moyenne de ses fourchettes × attaques par seconde) ;
    // le total compte le chaos, rappelé à part.
    let aps = it.props.get("aps").copied().unwrap_or(0.0);
    let tail = |v: f64| vec![(" (DPS ".to_string(), label), (format!("{v:.0}"), Color32::from_gray(225)), (")".to_string(), label)];
    let (mut total, mut chaos) = (0.0, 0.0);
    let mut rate_line = None;
    let mut out = Vec::new();
    for line in it.raw.lines().map(str::trim) {
        if let Some(c) = WEAPON_LINE.captures(line) {
            let kind = &c[1];
            let name = match kind {
                "Physical" => tr!("Dégâts Physiques"),
                "Chaos" => tr!("Dégâts de Chaos"),
                _ => tr!("Dégâts Élémentaires"),
            };
            let mut parts = vec![(tr!("{name} : ", name), label)];
            for (k, r) in RANGE_PART.captures_iter(&c[2]).enumerate() {
                if k > 0 {
                    parts.push((", ".into(), label));
                }
                let color = match kind {
                    "Physical" => if r.get(3).is_some() { aug } else { Color32::from_gray(225) },
                    "Elemental" => adds.iter().find(|(a, b, _)| *a == r[1] && *b == r[2]).map_or(Color32::from_gray(225), |(_, _, e)| damage_color(e)),
                    other => damage_color(other),
                };
                parts.push((format!("{}-{}", &r[1], &r[2]), color));
            }
            let dps: f64 = RANGE_PART.captures_iter(&c[2]).map(|r| (r[1].parse::<f64>().unwrap_or(0.0) + r[2].parse::<f64>().unwrap_or(0.0)) / 2.0).sum::<f64>() * aps;
            if dps > 0.0 {
                parts.extend(tail(dps));
                total += dps;
                if kind == "Chaos" {
                    chaos += dps;
                }
            }
            out.push(parts);
        } else if let Some(c) = RATE_LINE.captures(line) {
            let name = match &c[1] {
                "Critical Hit Chance" => tr!("Chances de Touche critique"),
                "Attacks per Second" => tr!("Attaques par seconde"),
                _ => tr!("Temps de rechargement"),
            };
            let parts = vec![(tr!("{name} : ", name), label), (c[2].to_string(), if c.get(3).is_some() { aug } else { Color32::from_gray(225) })];
            if &c[1] == "Attacks per Second" {
                rate_line = Some(out.len());
            }
            out.push(parts);
        }
    }
    // Total après les attaques par seconde (les lignes de dégâts peuvent venir après).
    if let Some(i) = rate_line.filter(|_| total > 0.0) {
        let mut t = tail(total);
        if chaos > 0.0 {
            t.pop();
            t.push((tr!(", dont ").into(), label));
            t.push((format!("{chaos:.0}"), Color32::from_gray(225)));
            t.push((tr!(" DPS chaos)").into(), label));
        }
        out[i].extend(t);
    }
    out
}

/// DPS total d'une arme d'attaque (physique, élémentaire et chaos), comme au bout de la ligne
/// des attaques par seconde ; `None` sans dégâts ni attaques par seconde.
fn weapon_total_dps(it: &Item) -> Option<f64> {
    let aps = it.props.get("aps").copied()?;
    let total: f64 = it
        .raw
        .lines()
        .filter_map(|l| WEAPON_LINE.captures(l.trim()))
        .flat_map(|c| RANGE_PART.captures_iter(&c[2]).map(|r| (r[1].parse::<f64>().unwrap_or(0.0) + r[2].parse::<f64>().unwrap_or(0.0)) / 2.0).collect::<Vec<_>>())
        .sum::<f64>()
        * aps;
    (total > 0.0).then_some(total)
}

/// Nom d'une compétence accordée : « Level 15 Freezing Shards (Max Level 18) » -> « Freezing Shards ».
fn skill_name(text: &str) -> &str {
    let text = text.split(" (").next().unwrap_or(text);
    match text.strip_prefix("Level ") {
        Some(rest) => rest.split_once(' ').map_or(rest, |(_, name)| name),
        None => text,
    }
    .trim()
}

/// « DPS 152 · physique 98 · élémentaire 54 ».
fn dps_parts(d: &compare::Dps) -> ui::Parts {
    let mut out: ui::Parts = vec![("DPS ".into(), false), (format!("{:.0}", d.total()), true)];
    if d.phys > 0.0 && d.ele > 0.0 {
        out.push((tr!(" · physique {phys} · élémentaire {ele}", phys = format!("{:.0}", d.phys), ele = format!("{:.0}", d.ele)), false));
    }
    out
}

/// « 3 » ou « 2.5 ».
fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}

/// Défenses en morceaux de texte, valeurs changées par les mods en bleu (`aug`).
fn defence_parts(d: [f64; 3], aug: [bool; 3]) -> ui::Parts {
    let mut out: ui::Parts = Vec::new();
    for ((n, v), a) in [tr!("Armure"), tr!("Score d'Évasion"), tr!("Bouclier d'énergie")].iter().zip(d).zip(aug).filter(|((_, v), _)| *v > 0.0) {
        if !out.is_empty() {
            out.push((" · ".into(), false));
        }
        out.push((format!("{n} "), false));
        out.push((format!("{v:.0}"), a));
    }
    out
}

/// Fioles et charmes : ce qu'ils récupèrent (ou leur durée) et leurs charges. Chaque valeur
/// vient avec un drapeau : changée par les mods (bleu).
fn flask_parts(recover: (f64, bool), mana: bool, time: (f64, bool), duration: (f64, bool), per_use: (f64, bool), max: (f64, bool)) -> Vec<ui::Parts> {
    let mut out = Vec::new();
    if recover.0 > 0.0 {
        out.push(vec![
            (tr!("Rend ").into(), false),
            (format!("{:.0}", recover.0), recover.1),
            (if mana { tr!(" Mana sur ") } else { tr!(" Vie sur ") }.into(), false),
            (fmt_num(time.0), time.1),
            (" s".into(), false),
        ]);
    } else if duration.0 > 0.0 {
        out.push(vec![(tr!("Dure ").into(), false), (fmt_num(duration.0), duration.1), (" s".into(), false)]);
    }
    if max.0 > 0.0 {
        out.push(vec![
            (tr!("Consomme ").into(), false),
            (format!("{:.0}", per_use.0), per_use.1),
            (tr!(" charges sur ").into(), false),
            (format!("{:.0}", max.0), max.1),
            (tr!(" par utilisation").into(), false),
        ]);
    }
    out
}

/// Taille minimale de la fenêtre.
const MIN_WINDOW: [f32; 2] = [1200.0, 1000.0];

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
    // Lancé depuis le dépôt (`target/debug` ou `target/release`) : version de développement, à
    // distinguer de l'application installée dans la barre de titre.
    let dev = std::env::current_exe().is_ok_and(|p| p.components().any(|c| c.as_os_str() == "target"));
    let title = if dev { "poe2-tracker-dev" } else { "poe2-tracker" };
    let mut viewport = egui::ViewportBuilder::default().with_title(title).with_app_id(desktop::APP_ID);
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
            .with_min_inner_size(MIN_WINDOW),
        ..Default::default()
    };
    eframe::run_native("poe2-tracker", options, Box::new(move |cc| Ok(Box::new(App::new(cc, dirs, stored, config)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_lines_against_worn() {
        let it = item::parse("Item Class: Charms\nRarity: Unique\nRite of Passage\nGolden Charm\n--------\nItem Level: 80\n--------\nPossessed by a random Spirit for 15 seconds on use\n+8% to Fire Resistance").unwrap();
        let mods = ["Possessed by a random Spirit for (10-20) seconds on use", "+(10-15)% to Fire Resistance", "Grants Onslaught"].map(String::from);
        let parts = unique_matches(&it, &mods);
        assert_eq!(parts[0], 1.5);
        assert!((parts[1] - 0.8).abs() < 1e-9);
        assert_eq!(parts[2], 0.0);
        assert_eq!([match_mark(parts[0]).0, match_mark(parts[1]).0, match_mark(parts[2]).0], ["✓", "~", "✗"]);
        assert_eq!(skill_name("Level 15 Freezing Shards (Max Level 18)"), "Freezing Shards");
        assert_eq!(skill_name("Freezing Shards"), "Freezing Shards");
        // Arme : fourchettes élémentaires rangées par élément d'après les mods.
        let staff = item::parse("Item Class: Quarterstaves\nRarity: Rare\nBramble Goad\nGuardian Quarterstaff\n--------\nPhysical Damage: 41-68 (augmented)\nElemental Damage: 4-87 (augmented)\nCritical Hit Chance: 10.00%\nAttacks per Second: 1.40\n--------\nItem Level: 70\n--------\nAdds 4 to 87 Lightning Damage").unwrap();
        let lines = weapon_lines(&staff);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1][1], ("4-87".to_string(), damage_color("Lightning")));
        // DPS au bout des lignes : physiques, élémentaires, total après les attaques par seconde.
        let text = |l: &Vec<(String, Color32)>| l.iter().map(|(t, _)| t.as_str()).collect::<String>();
        assert_eq!(text(&lines[0]), "Dégâts Physiques : 41-68 (DPS 76)");
        assert_eq!(text(&lines[1]), "Dégâts Élémentaires : 4-87 (DPS 64)");
        assert_eq!(text(&lines[3]), "Attaques par seconde : 1.40 (DPS 140)");
        // Ligne d'un seul élément (« Lightning Damage ») et chaos compté dans le total.
        let staff = item::parse("Item Class: Quarterstaves\nRarity: Rare\nBramble Goad\nGuardian Quarterstaff\n--------\nPhysical Damage: 52-86 (augmented)\nLightning Damage: 4-87 (augmented)\nChaos Damage: 10-20 (augmented)\nCritical Hit Chance: 10.00%\nAttacks per Second: 1.40\n--------\nItem Level: 70\n--------\nAdds 4 to 87 Lightning Damage").unwrap();
        let lines = weapon_lines(&staff);
        assert_eq!(text(&lines[1]), "Dégâts Élémentaires : 4-87 (DPS 64)");
        assert_eq!(text(&lines[2]), "Dégâts de Chaos : 10-20 (DPS 21)");
        assert_eq!(text(&lines[4]), "Attaques par seconde : 1.40 (DPS 181, dont 21 DPS chaos)");
    }
}
