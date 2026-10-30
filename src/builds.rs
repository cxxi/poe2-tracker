//! Builds suivis : chacun est un ensemble de fichiers .build (une étape par fichier),
//! importés depuis la page web du build (Mobalytics, Kami-labs). Les .build d'abord, puis ce
//! que l'API du site donne en plus (runes) : si cette API change, les .build restent.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::art::USER_AGENT;
use crate::guide;

type Res<T> = Result<T, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildMeta {
    pub id: String,
    pub name: String,
    pub source: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    builds: Vec<BuildMeta>,
    active: Option<String>,
    /// Build actif de chaque personnage.
    #[serde(default)]
    by_character: std::collections::HashMap<String, String>,
}

pub struct BuildStore {
    dir: PathBuf,
    index: Index,
    character: Option<String>,
}

/// Un fichier .build lu en mémoire : nom de fichier et contenu.
pub struct BuildFile {
    pub name: String,
    pub data: Vec<u8>,
}

impl BuildStore {
    pub fn open(data_dir: &Path) -> BuildStore {
        let dir = data_dir.join("builds");
        let index = std::fs::read_to_string(dir.join("builds.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        BuildStore { dir, index, character: None }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.dir.join("builds.json"), serde_json::to_string_pretty(&self.index).unwrap_or_default());
    }

    pub fn list(&self) -> &[BuildMeta] {
        &self.index.builds
    }

    /// Personnage courant : le build actif est mémorisé pour lui.
    pub fn set_character(&mut self, name: Option<&str>) {
        self.character = name.map(str::to_string);
    }

    pub fn active(&self) -> Option<&BuildMeta> {
        let by_char = self.character.as_ref().and_then(|c| self.index.by_character.get(c));
        let id = by_char.or(self.index.active.as_ref())?;
        self.index.builds.iter().find(|b| &b.id == id)
    }

    pub fn set_active(&mut self, id: &str) {
        self.index.active = Some(id.to_string());
        if let Some(c) = &self.character {
            self.index.by_character.insert(c.clone(), id.to_string());
        }
        self.save();
    }

    /// Déplace un build d'un cran dans la liste (`-1` : vers le haut, `1` : vers le bas).
    pub fn shift(&mut self, id: &str, by: isize) {
        let Some(i) = self.index.builds.iter().position(|b| b.id == id) else { return };
        let j = i as isize + by;
        if (0..self.index.builds.len() as isize).contains(&j) {
            self.index.builds.swap(i, j as usize);
            self.save();
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.index.builds.retain(|b| b.id != id);
        self.index.by_character.retain(|_, b| b != id);
        let _ = std::fs::remove_dir_all(self.dir.join(id));
        if self.index.active.as_deref() == Some(id) {
            self.index.active = self.index.builds.first().map(|b| b.id.clone());
        }
        self.save();
    }

    /// Dossier du build (ses étapes et ses réglages, comme les choix d'attributs).
    /// Page du build ouverte sur l'étape `label` si le site le permet, sinon la page du build.
    pub fn step_link(&self, id: &str, label: &str) -> Option<String> {
        let meta = self.list().iter().find(|b| b.id == id)?;
        let links: std::collections::BTreeMap<String, String> =
            std::fs::read(self.dir_of(id).join(STEP_LINKS))
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or_default();
        Some(
            links
                .get(label)
                .cloned()
                .unwrap_or_else(|| meta.source.clone()),
        )
    }

    pub fn dir_of(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    /// Fichiers .build du build, triés (Act 1, Act 2… Endgame Step 3).
    pub fn files(&self, id: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.dir.join(id))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "build"))
            .collect();
        files.sort();
        files
    }

    /// Enregistre un nouveau build à partir de ses fichiers et le rend actif.
    pub fn add(&mut self, files: Vec<BuildFile>, source: &str) -> Res<BuildMeta> {
        // On ne garde que les fichiers qui sont bien des étapes de build lisibles.
        let site = site_name(&files);
        let (steps, extras) = read_steps(files)?;
        let name = site.unwrap_or_else(|| guide::split_name(&steps[0].1.name).1);
        let id = format!("{}-{}", slug(&name), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        let dir = self.dir.join(&id);
        write_steps(&dir, &steps, extras)?;
        let meta = BuildMeta { id: id.clone(), name, source: source.to_string() };
        self.index.builds.push(meta.clone());
        self.set_active(&id);
        Ok(meta)
    }

    /// Remplace les fichiers d'un build (mise à jour depuis sa page) ; ses réglages, comme les
    /// choix d'attributs, sont gardés.
    pub fn update(&mut self, id: &str, files: Vec<BuildFile>, source: &str) -> Res<BuildMeta> {
        let site = site_name(&files);
        let (steps, extras) = read_steps(files)?;
        let dir = self.dir.join(id);
        for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|e| e == "build") || p.file_name().is_some_and(|n| EXTRAS.iter().any(|x| n == *x)) {
                let _ = std::fs::remove_file(p);
            }
        }
        write_steps(&dir, &steps, extras)?;
        let name = site.unwrap_or_else(|| guide::split_name(&steps[0].1.name).1);
        let meta = self.index.builds.iter_mut().find(|b| b.id == id).ok_or("build inconnu")?;
        meta.name = name;
        meta.source = source.to_string();
        let meta = meta.clone();
        self.save();
        Ok(meta)
    }
}

type Steps = Vec<(String, guide::RawBuild, Vec<u8>)>;

/// Nom complet donné par le site, s'il y en a un (sinon : celui des .build).
fn site_name(files: &[BuildFile]) -> Option<String> {
    files.iter().find(|f| f.name == BUILD_NAME).map(|f| String::from_utf8_lossy(&f.data).trim().to_string())
}

/// Étapes de build lisibles parmi des fichiers (dans l'ordre), et les runes s'il y en a.
fn read_steps(files: Vec<BuildFile>) -> Res<(Steps, Vec<(String, Vec<u8>)>)> {
    let extras = files.iter().filter(|f| EXTRAS.contains(&f.name.as_str())).map(|f| (f.name.clone(), f.data.clone())).collect();
    let mut steps: Steps = files
        .into_iter()
        .filter_map(|f| {
            let raw: guide::RawBuild = serde_json::from_slice(&f.data).ok()?;
            guide::split_name(&raw.name).0.as_ref()?;
            Some((f.name, raw, f.data))
        })
        .collect();
    if steps.is_empty() {
        return Err("aucune étape de build reconnue sur cette page".into());
    }
    steps.sort_by(|a, b| step_order(&a.1.name).cmp(&step_order(&b.1.name)));
    Ok((steps, extras))
}

fn write_steps(dir: &Path, steps: &Steps, extras: Vec<(String, Vec<u8>)>) -> Res<()> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (i, (_, raw, data)) in steps.iter().enumerate() {
        let label = guide::split_name(&raw.name).0.unwrap_or_default();
        let safe: String = label.chars().map(|c| if c.is_alphanumeric() || " .+-%".contains(c) { c } else { '_' }).collect();
        std::fs::write(dir.join(format!("{:02} - {safe}.build", i + 1)), data).map_err(|e| e.to_string())?;
    }
    for (name, data) in extras {
        std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ordre des étapes : tranches de niveaux, actes, endgame numéroté, puis variantes nommées
/// (dans l'ordre de l'archive, le tri étant stable).
fn step_order(name: &str) -> (u32, u32) {
    match guide::split_name(name).0 {
        Some(l) if guide::level_range(&l).is_some() => (0, guide::level_range(&l).map(|r| r.0).unwrap_or(0)),
        Some(l) if l.starts_with("Act ") => (1, l[4..].trim().parse().unwrap_or(0)),
        Some(l) if l.starts_with("Endgame Step ") => (2, l[13..].trim().parse().unwrap_or(0)),
        _ => (3, 0),
    }
}

fn slug(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Fichiers .build contenus dans un fichier .build ou une archive .zip.
pub fn read_build_files(name: &str, data: Vec<u8>) -> Res<Vec<BuildFile>> {
    if name.to_lowercase().ends_with(".zip") || data.starts_with(b"PK") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("archive illisible : {e}"))?;
        let mut out = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            if f.name().to_lowercase().ends_with(".build") {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                out.push(BuildFile { name: f.name().to_string(), data: buf });
            }
        }
        return Ok(out);
    }
    Ok(vec![BuildFile { name: name.to_string(), data }])
}

static KAMI_ZIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https://kami-labs\.fr/wp-content/uploads/poe2-builds/[^"'\s]+\.zip"#).unwrap());
/// Certaines pages n'ont pas d'archive mais des fichiers .build séparés.
static KAMI_BUILD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https://kami-labs\.fr/wp-content/uploads/poe2-builds/[^"'\s]+\.build"#).unwrap());

/// Identifiant du build sur Kami-labs (sert à trouver sa grille d'équipement).
static KAMI_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"data-build-id="(BUILD-\d+)""#).unwrap());

/// Runes recommandées (Kami-labs ou Mobalytics), à côté des fichiers .build du build.
pub const KAMI_RUNES: &str = "kami-runes.json";
/// Choix de récompenses de quête recommandés (Mobalytics), à côté des fichiers .build.
pub const QUEST_CHOICES: &str = "quest-choices.json";
/// Données enregistrées avec les .build, remplacées à chaque mise à jour.
const EXTRAS: [&str; 6] = [KAMI_RUNES, QUEST_CHOICES, SKILL_SETS, STEP_LINKS, ITEM_RARITIES, JEWELS];
/// Joyaux de l'arbre de talents par étape (Mobalytics), à côté des fichiers .build.
pub const JEWELS: &str = "jewels.json";
/// Rareté de chaque pièce recommandée (Mobalytics), à côté des fichiers .build.
pub const ITEM_RARITIES: &str = "item-rarities.json";
/// Set d'armes de chaque sort (Mobalytics), à côté des fichiers .build.
pub const SKILL_SETS: &str = "skill-sets.json";
/// Lien de la page du build ouverte sur chaque étape : nom de l'étape -> adresse.
pub const STEP_LINKS: &str = "step-links.json";

/// Récompense de quête que le build choisit, là où la campagne laisse le choix.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuestChoice {
    pub act: String,
    pub area: String,
    pub quest: String,
    pub reward: String,
}

/// Choix de récompenses de quête d'un build, si on les connaît.
pub fn load_quest_choices(dir: &Path) -> Vec<QuestChoice> {
    std::fs::read(dir.join(QUEST_CHOICES)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}
/// Nom complet du build quand le site le donne (celui des .build est coupé à 40 caractères).
const BUILD_NAME: &str = "build-name.txt";

const MOBALYTICS_API: &str = "https://mobalytics.gg/api/poe-2/v1/graphql/query";
/// Lien public d'un build Mobalytics, ou lien sur le profil de son auteur.
static MOBA_PUBLIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"mobalytics\.gg/poe-2/builds/([^/?#]+)").unwrap());
static MOBA_PROFILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"mobalytics\.gg/poe-2/profile/([^/?#]+)/builds/([^/?#]+)").unwrap());

/// Ce que l'on demande à Mobalytics : titre de chaque variante (« Act 1 {1} »), runes de
/// chaque pièce (sets d'armes compris) et set d'armes de chaque sort.
const MOBA_DOC: &str = "error data {
    id
    data { name }
    content: contentV2 { id ... on NgfDocumentCmWidgetContentVariantsV1 { data { childrenVariants { id title } } } }
    ... on Poe2UserGeneratedDocument { data {
    questRewards { quests { quest { name act area } reward { bakedDescription } } }
    buildVariants { values { id passiveTree { jewels { iconURL isUnique jewelSlug nodeSlug prefixSlugs suffixSlugs } }
    skillGems { gems { weaponSet activeSkill { name } } } equipment {
        helmet { runes { slug } commonItem { ...affixes } } body { runes { slug } commonItem { ...affixes } }
        gloves { runes { slug } commonItem { ...affixes } } boots { runes { slug } commonItem { ...affixes } }
        mainHand { set1 { runes { slug } commonItem { ...affixes } } set2 { runes { slug } commonItem { ...affixes } } }
        offHand { set1 { runes { slug } commonItem { ...affixes } } set2 { runes { slug } commonItem { ...affixes } } }
        belt { commonItem { ...affixes } } amulet { commonItem { ...affixes } }
        leftRing { commonItem { ...affixes } } rightRing { commonItem { ...affixes } }
        flask1 { commonItem { ...affixes } } flask2 { commonItem { ...affixes } }
        charm1 { commonItem { ...affixes } } charm2 { commonItem { ...affixes } } charm3 { commonItem { ...affixes } }
    } } } } }
}";

/// Préfixes et suffixes d'une pièce : Mobalytics en tire sa rareté (voir `mobalytics_rarities`).
const MOBA_AFFIXES: &str = "fragment affixes on Poe2DocumentUgWidgetEquipmentCommonV1 { isUnique prefixes { slug } suffixes { slug } }";

/// Requête au service GraphQL de Mobalytics (celui de leur site).
fn mobalytics_query(query: &str, variables: serde_json::Value) -> Res<serde_json::Value> {
    ureq::post(MOBALYTICS_API)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(30))
        .send_json(serde_json::json!({ "query": query, "variables": variables }))
        .map_err(|e| format!("Mobalytics injoignable : {e}"))?
        .into_json()
        .map_err(|e| e.to_string())
}

/// Document Mobalytics d'un build (identifiant, variantes, runes), et le champ qui l'a donné.
fn mobalytics_document(page_url: &str) -> Res<(serde_json::Value, &'static str)> {
    let (field, input_type, input) = if let Some(c) = MOBA_PROFILE.captures(page_url) {
        ("userGeneratedDocumentBySlugifiedName", "NgfUserGeneratedDocumentInputBySlugifiedName", serde_json::json!({ "authorProfileName": &c[1], "slugifiedName": &c[2] }))
    } else if let Some(c) = MOBA_PUBLIC.captures(page_url) {
        ("userGeneratedDocumentBySlug", "Poe2UserGeneratedDocumentInputBySlug", serde_json::json!({ "slug": &c[1], "type": "builds", "widgetsOverride": [] }))
    } else {
        return Err("lien Mobalytics non reconnu (attendu : mobalytics.gg/poe-2/builds/… ou …/profile/<auteur>/builds/…)".into());
    };
    let query = format!("query Build($input: {input_type}!) {{ game: poe2 {{ documents {{ {field}(input: $input) {{ {MOBA_DOC} }} }} }} }} {MOBA_AFFIXES}");
    let resp = mobalytics_query(&query, serde_json::json!({ "input": input }))?;
    if resp["data"]["game"]["documents"][field]["data"]["id"].as_str().is_none() {
        return Err("build introuvable sur Mobalytics (lien ou format changé ?)".into());
    }
    Ok((resp, field))
}

/// Build Mobalytics : un .build par variante (l'export du site vers le planificateur du jeu),
/// puis les runes.
fn fetch_mobalytics(page_url: &str) -> Res<Vec<BuildFile>> {
    let (resp, field) = mobalytics_document(page_url)?;
    let doc = &resp["data"]["game"]["documents"][field]["data"];
    let id = doc["id"].as_str().unwrap_or_default();
    let variants: Vec<(&str, &str)> = doc["content"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|c| c["data"]["childrenVariants"].as_array().into_iter().flatten())
        .filter_map(|v| Some((v["id"].as_str()?, v["title"].as_str()?)))
        .collect();
    if variants.is_empty() {
        return Err("aucune variante sur ce build Mobalytics".into());
    }
    const EXPORT: &str = "query Export($input: Poe2UserGeneratedDocumentInputById!, $variantId: String!) { poe2 { documents { userGeneratedDocumentById(input: $input) { error data { exportToGame(variantId: $variantId) } } } } }";
    let mut files = Vec::new();
    for (vid, title) in variants {
        let r = mobalytics_query(EXPORT, serde_json::json!({ "input": { "id": id }, "variantId": vid }))?;
        let Some(text) = r["data"]["poe2"]["documents"]["userGeneratedDocumentById"]["data"]["exportToGame"].as_str() else { continue };
        files.push(BuildFile { name: format!("{title}.build"), data: text.as_bytes().to_vec() });
    }
    if let Some(steps) = mobalytics_runes(&resp, field) {
        files.push(BuildFile { name: KAMI_RUNES.into(), data: serde_json::to_vec(&steps).map_err(|e| e.to_string())? });
    }
    if let Some(steps) = mobalytics_rarities(&resp, field) {
        files.push(BuildFile { name: ITEM_RARITIES.into(), data: serde_json::to_vec(&steps).map_err(|e| e.to_string())? });
    }
    if let Some(mut steps) = mobalytics_jewels(&resp, field) {
        mobalytics_unique_jewels(&mut steps);
        files.push(BuildFile { name: JEWELS.into(), data: serde_json::to_vec(&steps).map_err(|e| e.to_string())? });
    }
    if let Some(steps) = mobalytics_skill_sets(&resp, field) {
        files.push(BuildFile { name: SKILL_SETS.into(), data: serde_json::to_vec(&steps).map_err(|e| e.to_string())?,
        });
    }
    let links = mobalytics_step_links(doc, page_url);
    if !links.is_empty() {
        files.push(BuildFile {
            name: STEP_LINKS.into(),
            data: serde_json::to_vec(&links).map_err(|e| e.to_string())? });
    }
    let quests: Vec<QuestChoice> = doc["data"]["questRewards"]["quests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let s = |v: &serde_json::Value| v.as_str().map(str::to_string);
            Some(QuestChoice { act: s(&q["quest"]["act"])?, area: s(&q["quest"]["area"])?, quest: s(&q["quest"]["name"])?, reward: s(&q["reward"]["bakedDescription"])? })
        })
        .collect();
    if !quests.is_empty() {
        files.push(BuildFile { name: QUEST_CHOICES.into(), data: serde_json::to_vec(&quests).map_err(|e| e.to_string())? });
    }
    if let Some(name) = doc["data"]["name"].as_str().filter(|n| !n.is_empty()) {
        files.push(BuildFile { name: BUILD_NAME.into(), data: name.as_bytes().to_vec() });
    }
    Ok(files)
}

/// Runes par étape dans la réponse de Mobalytics (identifiants « soulcore-… », traduits en noms
/// à l'affichage).
fn mobalytics_runes(resp: &serde_json::Value, field: &str) -> Option<Vec<StepRunes>> {
    let doc = &resp["data"]["game"]["documents"][field]["data"];
    let titles = variant_titles(doc)?;
    let runes = |v: &serde_json::Value| -> Vec<String> { v["runes"].as_array().into_iter().flatten().filter_map(|r| r["slug"].as_str().map(str::to_string)).collect() };
    let steps = doc["data"]["buildVariants"]["values"]
        .as_array()?
        .iter()
        .filter_map(|v| {
            let name = titles.get(v["id"].as_str()?)?.clone();
            let e = &v["equipment"];
            // Mêmes clés que la grille Kami-labs (voir `kami_slot`), plus le set d'armes II.
            let slots = [
                ("helmet", &e["helmet"]),
                ("bodyArmour", &e["body"]),
                ("gloves", &e["gloves"]),
                ("boots", &e["boots"]),
                ("weapon1", &e["mainHand"]["set1"]),
                ("weapon2", &e["offHand"]["set1"]),
                ("mainHand2", &e["mainHand"]["set2"]),
                ("offHand2", &e["offHand"]["set2"]),
            ];
            let items = slots.into_iter().map(|(k, v)| (k.to_string(), runes(v))).filter(|(_, r)| !r.is_empty()).collect();
            Some(StepRunes { name, items })
        })
        .collect();
    Some(steps)
}

/// Rareté des pièces d'une étape, comme Mobalytics l'affiche : unique, sinon d'après le plus grand
/// nombre de préfixes ou de suffixes (2 et plus : rare, 1 : magique, aucun : normal).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepRarities {
    pub name: String,
    pub items: std::collections::BTreeMap<crate::gear::Slot, crate::item::Rarity>,
}

fn mobalytics_rarities(resp: &serde_json::Value, field: &str) -> Option<Vec<StepRarities>> {
    use crate::gear::Slot;
    use crate::item::Rarity;
    let doc = &resp["data"]["game"]["documents"][field]["data"];
    let titles = variant_titles(doc)?;
    let rarity = |v: &serde_json::Value| -> Option<Rarity> {
        let c = v["commonItem"].as_object()?;
        if c.get("isUnique").and_then(|u| u.as_bool()) == Some(true) {
            return Some(Rarity::Unique);
        }
        let count = |k: &str| c.get(k).and_then(|a| a.as_array()).map_or(0, Vec::len);
        Some(match count("prefixes").max(count("suffixes")) {
            0 => Rarity::Normal,
            1 => Rarity::Magic,
            _ => Rarity::Rare,
        })
    };
    let steps = doc["data"]["buildVariants"]["values"]
        .as_array()?
        .iter()
        .filter_map(|v| {
            let name = titles.get(v["id"].as_str()?)?.clone();
            let e = &v["equipment"];
            let slots = [
                (Slot::Helmet, &e["helmet"]),
                (Slot::Body, &e["body"]),
                (Slot::Gloves, &e["gloves"]),
                (Slot::Boots, &e["boots"]),
                (Slot::Weapon, &e["mainHand"]["set1"]),
                (Slot::Offhand, &e["offHand"]["set1"]),
                (Slot::Weapon2, &e["mainHand"]["set2"]),
                (Slot::Offhand2, &e["offHand"]["set2"]),
                (Slot::Belt, &e["belt"]),
                (Slot::Amulet, &e["amulet"]),
                (Slot::RingLeft, &e["leftRing"]),
                (Slot::RingRight, &e["rightRing"]),
                (Slot::LifeFlask, &e["flask1"]),
                (Slot::ManaFlask, &e["flask2"]),
                (Slot::Charm1, &e["charm1"]),
                (Slot::Charm2, &e["charm2"]),
                (Slot::Charm3, &e["charm3"]),
            ];
            let items = slots.into_iter().filter_map(|(s, v)| Some((s, rarity(v)?))).collect();
            Some(StepRarities { name, items })
        })
        .collect();
    Some(steps)
}

/// Joyau posé dans une châsse de l'arbre de talents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Jewel {
    /// Numéro du nœud de la châsse (« skill » de l'arbre PoB).
    pub node: u32,
    /// Image dans les archives du jeu (« Art/2DItems/Jewels/SapphireJewel.dds »).
    pub icon: String,
    pub name: String,
    pub rarity: crate::item::Rarity,
    /// Identifiants des mods visés (préfixes puis suffixes), traduits à l'affichage.
    pub mods: Vec<String>,
    /// Joyau unique : son type de base et le texte de ses mods (données de Mobalytics), ou ses
    /// emplacements libres quand le build n'en choisit pas (« Custom Desecrated prefix »).
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub texts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StepJewels {
    pub name: String,
    pub jewels: Vec<Jewel>,
}

/// Nom d'un joyau d'après son identifiant Mobalytics : type de base pour un joyau magique ou
/// rare (« jewel-jewelint » : Sapphire), nom de l'unique sinon (« …-heart-of-the-well »).
fn jewel_name(slug: &str, unique: bool) -> String {
    let base = match slug {
        "jewel-jewelint" => "Sapphire",
        "jewel-jewelstr" => "Ruby",
        "jewel-jeweldex" => "Emerald",
        "jewel-jewelradiusint" => "Time-Lost Sapphire",
        "jewel-jewelradiusstr" => "Time-Lost Ruby",
        "jewel-jewelradiusdex" => "Time-Lost Emerald",
        "jewel-jeweldiamond" => "Diamond",
        "jewel-jewelradiusdiamond" => "Time-Lost Diamond",
        _ => "",
    };
    if !base.is_empty() && !unique {
        return base.to_string();
    }
    // « jewel-fouruniquejewel12-heart-of-the-well » : les mots après l'identifiant interne.
    let words: Vec<&str> = slug.split('-').skip(2).collect();
    let name = words
        .iter()
        .map(|w| if ["of", "the", "and", "in"].contains(w) { w.to_string() } else { w[..1].to_uppercase() + &w[1..] })
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() { slug.to_string() } else { name[..1].to_uppercase() + &name[1..] }
}

fn mobalytics_jewels(resp: &serde_json::Value, field: &str) -> Option<Vec<StepJewels>> {
    use crate::item::Rarity;
    let doc = &resp["data"]["game"]["documents"][field]["data"];
    let titles = variant_titles(doc)?;
    let steps = doc["data"]["buildVariants"]["values"]
        .as_array()?
        .iter()
        .filter_map(|v| {
            let name = titles.get(v["id"].as_str()?)?.clone();
            let jewels = v["passiveTree"]["jewels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|j| {
                    let node = j["nodeSlug"].as_str()?.strip_prefix("node-")?.parse().ok()?;
                    let unique = j["isUnique"].as_bool().unwrap_or(false);
                    let slugs = |k: &str| -> Vec<String> { j[k].as_array().into_iter().flatten().filter_map(|s| s.as_str()).filter(|s| !s.is_empty()).map(String::from).collect() };
                    let (prefixes, suffixes) = (slugs("prefixSlugs"), slugs("suffixSlugs"));
                    let rarity = match (unique, prefixes.len().max(suffixes.len())) {
                        (true, _) => Rarity::Unique,
                        (_, 0) => Rarity::Normal,
                        (_, 1) => Rarity::Magic,
                        _ => Rarity::Rare,
                    };
                    // « https://cdn…/game/Art/2DItems/Jewels/SapphireJewel.avif » -> chemin du jeu.
                    let url = j["iconURL"].as_str().unwrap_or_default();
                    let path = url.split("/game/").nth(1).unwrap_or_default();
                    let icon = path.rsplit_once('.').map_or(path.to_string(), |(p, _)| format!("{p}.dds"));
                    // Mods d'unique : identifiants propres à Mobalytics, traduits par `mobalytics_unique_jewels`.
                    let mods = prefixes.into_iter().chain(suffixes).collect();
                    let slug = j["jewelSlug"].as_str().unwrap_or_default();
                    // L'identifiant du joyau, gardé dans `base` le temps de le résoudre.
                    let base = if unique { slug.to_string() } else { String::new() };
                    Some(Jewel { node, icon, name: jewel_name(slug, unique), rarity, mods, base, texts: vec![] })
                })
                .collect();
            Some(StepJewels { name, jewels })
        })
        .collect();
    Some(steps)
}

/// Joyaux uniques : type de base et texte de leurs mods, lus dans les données de Mobalytics
/// (un unique « Desecrated » a des mods propres au site). Sans réponse, ils gardent leur nom.
fn mobalytics_unique_jewels(steps: &mut [StepJewels]) {
    let uniques: Vec<&Jewel> = steps.iter().flat_map(|s| &s.jewels).filter(|j| j.rarity == crate::item::Rarity::Unique).collect();
    if uniques.is_empty() {
        return;
    }
    let mut slugs: Vec<String> = uniques.iter().map(|j| j.base.clone()).collect();
    slugs.sort();
    slugs.dedup();
    let mut mods: Vec<String> = uniques.iter().flat_map(|j| j.mods.clone()).collect();
    mods.sort();
    mods.dedup();
    const STATIC: &str = "query Jewels($jewels: [String!]!, $mods: [String!]!) { game: poe2 { staticData {
        poe2Jewels(filter: { slug: { values: $jewels } }) { data { slug bakedDescriptions baseItemType { slug } } }
        poe2Prefixes(filter: { slug: { values: $mods } }) { data { slug combinedDescription } } } } }";
    let Ok(resp) = mobalytics_query(STATIC, serde_json::json!({ "jewels": slugs, "mods": mods })) else { return };
    let data = &resp["data"]["game"]["staticData"];
    let texts: std::collections::HashMap<&str, &str> = data["poe2Prefixes"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| Some((m["slug"].as_str()?, m["combinedDescription"].as_str()?)))
        .collect();
    let jewels: std::collections::HashMap<&str, &serde_json::Value> =
        data["poe2Jewels"]["data"].as_array().into_iter().flatten().filter_map(|j| Some((j["slug"].as_str()?, j))).collect();
    for j in steps.iter_mut().flat_map(|s| s.jewels.iter_mut()).filter(|j| j.rarity == crate::item::Rarity::Unique) {
        let info = jewels.get(j.base.as_str());
        // « jeweldiamond » -> type de base lisible.
        let base = info.and_then(|i| i["baseItemType"]["slug"].as_str()).unwrap_or_default();
        j.base = jewel_name(&format!("jewel-{base}"), false);
        if j.base.starts_with("jewel-") || j.base.is_empty() {
            j.base = base.trim_start_matches("jewel").to_string();
            if let Some(f) = j.base.get(..1) {
                j.base = f.to_uppercase() + &j.base[1..];
            }
        }
        j.texts = j.mods.iter().filter_map(|m| texts.get(m.as_str()).map(|t| t.replace('\u{2013}', "-"))).collect();
        if j.texts.is_empty() {
            j.texts = info.and_then(|i| i["bakedDescriptions"].as_array()).into_iter().flatten().filter_map(|d| d.as_str().map(String::from)).collect();
        }
        j.mods.clear();
    }
}

/// Joyaux de l'arbre d'un build, si on les connaît (Mobalytics).
pub fn load_jewels(dir: &Path) -> Vec<StepJewels> {
    std::fs::read(dir.join(JEWELS)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

/// Rareté des pièces recommandées d'un build, si on la connaît (Mobalytics).
pub fn load_rarities(dir: &Path) -> Vec<StepRarities> {
    std::fs::read(dir.join(ITEM_RARITIES)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

/// Étiquette d'étape de chaque variante Mobalytics, par identifiant (« Act 1 {1} » → « Act 1 »).
fn variant_titles(doc: &serde_json::Value) -> Option<std::collections::HashMap<&str, String>> {
    Some(
        doc["content"]
            .as_array()?
            .iter()
            .flat_map(|c| c["data"]["childrenVariants"].as_array().into_iter().flatten())
            .filter_map(|v| {
                let title = v["title"].as_str()?;
                Some((v["id"].as_str()?, title.split(" {").next().unwrap_or(title).trim().to_string()))
            })
            .collect(),
    )
}

/// Lien vers chaque étape : la page garde la variante ouverte dans un paramètre
/// « ws-ngf5-<bloc des variantes>=activeVariantId,<variante> ».
fn mobalytics_step_links(
    doc: &serde_json::Value,
    page_url: &str,
) -> std::collections::BTreeMap<String, String> {
    let base = page_url.split(['?', '#']).next().unwrap_or(page_url);
    let mut links = std::collections::BTreeMap::new();
    for widget in doc["content"].as_array().into_iter().flatten() {
        let Some(wid) = widget["id"].as_str() else {
            continue;
        };
        for v in widget["data"]["childrenVariants"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let (Some(vid), Some(title)) = (v["id"].as_str(), v["title"].as_str()) else {
                continue;
            };
            let label = title.split(" {").next().unwrap_or(title).trim().to_string();
            links
                .entry(label)
                .or_insert_with(|| format!("{base}?ws-ngf5-{wid}=activeVariantId%2C{vid}"));
        }
    }
    links
}

/// Set d'armes de chaque sort par étape dans la réponse de Mobalytics. Les sorts sans set
/// (esprit, ou non précisé) n'y sont pas.
fn mobalytics_skill_sets(resp: &serde_json::Value, field: &str) -> Option<Vec<StepSkillSets>> {
    let doc = &resp["data"]["game"]["documents"][field]["data"];
    let titles = variant_titles(doc)?;
    let steps: Vec<StepSkillSets> = doc["data"]["buildVariants"]["values"]
        .as_array()?
        .iter()
        .filter_map(|v| {
            let name = titles.get(v["id"].as_str()?)?.clone();
            let sets = v["skillGems"]["gems"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| {
                    // Sans set assigné (null), le sort s'utilise avec les deux.
                    let set = match g["weaponSet"].as_str() {
                        Some("set1") => 1,
                        Some("set2") => 2,
                        _ => 0,
                    };
                    Some((g["activeSkill"]["name"].as_str()?.to_string(), set))
                })
                .collect();
            Some(StepSkillSets { name, sets })
        })
        .collect();
    steps.iter().any(|s| !s.sets.is_empty()).then_some(steps)
}

/// Set d'armes des sorts d'une étape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepSkillSets {
    pub name: String,
    /// Nom du sort -> set d'armes (1 ou 2 ; 0 : les deux).
    pub sets: std::collections::BTreeMap<String, u8>,
}

/// Sets d'armes des sorts d'un build, si on les connaît.
pub fn load_skill_sets(dir: &Path) -> Vec<StepSkillSets> {
    std::fs::read(dir.join(SKILL_SETS)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

/// Runes d'une pièce recommandées par le build, par étape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepRunes {
    pub name: String,
    /// Emplacement (clé Kami-labs : "weapon1", "bodyArmour"…) -> runes, une par châsse.
    pub items: std::collections::BTreeMap<String, Vec<String>>,
}

/// Emplacement d'une clé de la grille Kami-labs (seules les pièces à châsses comptent).
pub fn kami_slot(key: &str) -> Option<crate::gear::Slot> {
    use crate::gear::Slot;
    Some(match key {
        "weapon1" => Slot::Weapon,
        "weapon2" => Slot::Offhand,
        "helmet" => Slot::Helmet,
        "bodyArmour" => Slot::Body,
        "gloves" => Slot::Gloves,
        "boots" => Slot::Boots,
        "belt" => Slot::Belt,
        "amulet" => Slot::Amulet,
        "ring1" => Slot::RingLeft,
        "ring2" => Slot::RingRight,
        "mainHand2" => Slot::Weapon2,
        "offHand2" => Slot::Offhand2,
        _ => return None,
    })
}

/// Grille d'équipement d'une page Kami-labs (les données de la page : runes, source du build),
/// trouvée par l'identifiant du build écrit dans la page.
fn kami_grid(page: &str) -> Option<serde_json::Value> {
    let id = &KAMI_ID.captures(page)?[1];
    let grid = String::from_utf8_lossy(&download(&format!("https://kami-labs.fr/wp-content/uploads/poe2-builds/{id}/equipment-grid.html")).ok()?).into_owned();
    kami_grid_data(&grid)
}

/// L'objet JSON `{ …, "steps": [ … ] }` écrit dans le script de la grille.
fn kami_grid_data(grid: &str) -> Option<serde_json::Value> {
    let at = grid.find("\"steps\": [").or_else(|| grid.find("\"steps\":["))?;
    let start = grid[..at].rfind('{')?;
    serde_json::Deserializer::from_str(&grid[start..]).into_iter().next()?.ok()
}

/// Build repris de Mobalytics : lien de sa page d'origine.
fn kami_source(data: &serde_json::Value) -> Option<String> {
    let url = data["sourceUrl"].as_str()?;
    (data["source"].as_str() == Some("mobalytics") && url.contains("mobalytics.gg")).then(|| url.to_string())
}

/// Runes par étape dans les données de la grille (les fichiers .build n'en contiennent pas).
fn kami_runes(data: &serde_json::Value) -> Option<Vec<StepRunes>> {
    let steps = data["steps"]
        .as_array()?
        .iter()
        .map(|st| {
            // « Act 1 {1} » -> « Act 1 », comme les étiquettes des fichiers .build.
            let name = st["name"].as_str().unwrap_or_default();
            let name = name.split(" {").next().unwrap_or(name).trim().to_string();
            let items = st["equipment"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| {
                    let runes: Vec<String> = v["runes"].as_array()?.iter().filter_map(|r| r["name_en"].as_str().or(r["name"].as_str()).map(str::to_string)).collect();
                    (!runes.is_empty()).then(|| (k.clone(), runes))
                })
                .collect();
            StepRunes { name, items }
        })
        .collect();
    Some(steps)
}

/// Runes recommandées d'un build, si on les connaît.
pub fn load_runes(dir: &Path) -> Vec<StepRunes> {
    std::fs::read(dir.join(KAMI_RUNES)).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

fn download(url: &str) -> Res<Vec<u8>> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(403, _) => "le site refuse l'accès aux logiciels (403)".to_string(),
            e => e.to_string(),
        })?;
    let mut buf = Vec::new();
    resp.into_reader().take(50_000_000).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Fichiers d'un build à partir de sa page web (Mobalytics, Kami-labs) ou d'un lien direct
/// vers un .build / .zip.
pub fn fetch_url(url: &str) -> Res<Vec<BuildFile>> {
    let url = url.trim();
    let lower = url.to_lowercase();
    if lower.contains("mobalytics.gg") {
        return fetch_mobalytics(url);
    }
    if lower.contains("maxroll.gg") {
        return Err("Maxroll n'est pas encore pris en charge : lien Mobalytics ou Kami-labs".into());
    }
    if lower.ends_with(".build") || lower.ends_with(".zip") {
        let name = url.rsplit('/').next().unwrap_or("build.zip").to_string();
        return read_build_files(&name, download(url)?);
    }
    if lower.contains("kami-labs.fr") {
        let page = String::from_utf8_lossy(&download(url)?).into_owned();
        let grid = kami_grid(&page);
        // Build repris de Mobalytics : ses données y sont plus complètes et plus stables. Kami-labs
        // reste le secours si Mobalytics ne répond pas.
        if let Some(source) = grid.as_ref().and_then(kami_source) {
            if let Ok(files) = fetch_mobalytics(&source) {
                return Ok(files);
            }
        }
        // Runes recommandées : dans la grille d'équipement de la page, pas dans les .build.
        let runes = grid
            .as_ref()
            .and_then(kami_runes)
            .and_then(|steps| serde_json::to_vec(&steps).ok())
            .map(|data| BuildFile { name: KAMI_RUNES.into(), data });
        if let Some(zip_url) = KAMI_ZIP.find(&page) {
            let mut files = read_build_files(zip_url.as_str(), download(zip_url.as_str())?)?;
            files.extend(runes);
            return Ok(files);
        }
        let mut urls: Vec<&str> = KAMI_BUILD.find_iter(&page).map(|m| m.as_str()).collect();
        urls.sort();
        urls.dedup();
        if urls.is_empty() {
            return Err("pas de fichier de build sur cette page Kami-labs".into());
        }
        let mut files = Vec::new();
        for u in urls {
            let name = u.rsplit('/').next().unwrap_or("etape.build");
            files.extend(read_build_files(name, download(u)?)?);
        }
        files.extend(runes);
        return Ok(files);
    }
    Err("lien non pris en charge : page d'un build Mobalytics ou Kami-labs".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Joyaux d'un vrai build Mobalytics (réseau) : `cargo test mobalytics_jewels_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mobalytics_jewels_live() {
        let url = "https://mobalytics.gg/poe-2/profile/pathofky/builds/cold-caster-chronomancer-leaguestart-to-endgame";
        let (resp, field) = mobalytics_document(url).unwrap();
        let mut steps = mobalytics_jewels(&resp, field).unwrap();
        mobalytics_unique_jewels(&mut steps);
        for s in steps.iter().filter(|s| !s.jewels.is_empty()) {
            for j in &s.jewels {
                eprintln!("{} | {} {} {:?} | {:?} {:?}", s.name, j.name, j.base, j.rarity, j.mods, j.texts);
            }
        }
    }

    #[test]
    fn kami_grid_runes() {
        let grid = r#"<script> function load() { return {
            "buildId": "x",
            "steps": [ { "index": 0, "name": "Act 1 {1}", "equipment": {
                "weapon1": { "name": "Gelid Staff", "sockets": 2, "runes": [ { "name": "Rune du Fer", "name_en": "Iron Rune" }, { "name_en": "Desert Rune" } ] },
                "boots": { "name": "Sandals", "sockets": 0, "runes": [] } } } ]
        }; } </script>"#;
        let data = kami_grid_data(grid).unwrap();
        assert_eq!(kami_source(&data), None);
        let steps = kami_runes(&data).unwrap();
        assert_eq!(steps[0].name, "Act 1");
        assert_eq!(steps[0].items["weapon1"], ["Iron Rune", "Desert Rune"]);
        assert!(!steps[0].items.contains_key("boots"));
    }

    #[test]
    fn mobalytics_response_runes() {
        let resp = serde_json::json!({ "data": { "game": { "documents": { "userGeneratedDocumentBySlug": { "error": null, "data": {
            "content": [ {}, { "data": { "childrenVariants": [ { "id": "a", "title": "Act 1 {1}" }, { "id": "b", "title": "Endgame Step 3 {8}" } ] } } ],
            "data": { "buildVariants": { "values": [
                { "id": "a", "equipment": { "boots": { "runes": null }, "mainHand": { "set1": { "runes": [] }, "set2": null } } },
                { "id": "b", "equipment": { "boots": { "runes": [ { "slug": "soulcore-runespecial9" } ] }, "mainHand": { "set2": { "runes": [ { "slug": "soulcore-x" } ] } } } }
            ] } }
        } } } } } });
        let steps = mobalytics_runes(&resp, "userGeneratedDocumentBySlug").unwrap();
        assert_eq!(steps[0].name, "Act 1");
        assert!(steps[0].items.is_empty());
        assert_eq!(steps[1].name, "Endgame Step 3");
        assert_eq!(steps[1].items["boots"], ["soulcore-runespecial9"]);
        assert_eq!(kami_slot("mainHand2"), Some(crate::gear::Slot::Weapon2));
        assert_eq!(steps[1].items["mainHand2"], ["soulcore-x"]);
    }

    #[test]
    fn mobalytics_response_skill_sets() {
        let resp = serde_json::json!({ "data": { "game": { "documents": { "userGeneratedDocumentBySlug": { "error": null, "data": {
            "content": [ { "data": { "childrenVariants": [ { "id": "a", "title": "Act 3 {3}" } ] } } ],
            "data": { "buildVariants": { "values": [ { "id": "a", "skillGems": { "gems": [
                { "weaponSet": "set1", "activeSkill": { "name": "Ice Nova" } },
                { "weaponSet": "set2", "activeSkill": { "name": "Elemental Weakness" } },
                { "weaponSet": "all", "activeSkill": { "name": "Sigil of Power" } },
                { "weaponSet": null, "activeSkill": { "name": "Mana Remnants" } }
            ] } } ] } }
        } } } } } });
        let steps = mobalytics_skill_sets(&resp, "userGeneratedDocumentBySlug").unwrap();
        assert_eq!(steps[0].name, "Act 3");
        let sets: Vec<(&str, u8)> = steps[0].sets.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        // Sans set assigné : les deux sets (0), comme « all ».
        assert_eq!(sets, [("Elemental Weakness", 2), ("Ice Nova", 1), ("Mana Remnants", 0), ("Sigil of Power", 0)]);
    }

    /// Réseau : runes de la page Kami-labs du build d'Ulf.
    /// Réseau : la page Kami-labs du build d'Ulf indique sa source Mobalytics, et ses runes.
    #[test]
    #[ignore]
    fn kami_page_source_and_runes() {
        let url = "https://kami-labs.fr/path-of-exile-2/builds-path-of-exile-2/build-guerrier-earthshatter-brutus-warbringer-saison-5/";
        let page = String::from_utf8_lossy(&download(url).unwrap()).into_owned();
        let data = kami_grid(&page).unwrap();
        let source = kami_source(&data);
        eprintln!("source : {source:?}");
        assert!(source.is_some_and(|s| s.contains("mobalytics.gg/poe-2/builds/")));
        assert!(kami_runes(&data).unwrap().iter().any(|s| s.name == "BUDGET" && s.items.contains_key("bodyArmour")));
        // L'import passe alors par Mobalytics : le nom complet du guide vient de son API.
        let files = fetch_url(url).unwrap();
        assert!(files.iter().any(|f| f.name == BUILD_NAME));
    }

    #[test]
    fn order_and_slug() {
        assert!(step_order("Act 2 {2} - X") < step_order("Act 10 {10} - X"));
        assert!(step_order("Act 5 {5} - X") < step_order("Endgame Step 1 {6} - X"));
        assert_eq!(slug("Cold Caster Chronomancer"), "cold-caster-chronomancer");
    }

    /// Réseau : page Kami-labs sans archive, avec un fichier .build par tranche de niveaux.
    #[test]
    #[ignore]
    fn kami_page_with_separate_build_files() {
        let files = fetch_url("https://kami-labs.fr/path-of-exile-2/builds-path-of-exile-2/build-guerrier-earthshatter-brutus-warbringer-saison-5/").unwrap();
        let dir = std::env::temp_dir().join(format!("poe2-tracker-kami-{}", std::process::id()));
        let mut store = BuildStore::open(&dir);
        let meta = store.add(files, "test").unwrap();
        let steps = crate::guide::load_steps(&store.files(&meta.id), &crate::guide::Catalog::default());
        eprintln!("{} : {:?}", meta.name, steps.iter().map(|s| &s.label).collect::<Vec<_>>());
        assert_eq!(steps[0].label, "Niv. 1-21");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn maxroll_is_explained() {
        let err = fetch_url("https://maxroll.gg/poe2/build-guides/x").err().unwrap();
        assert!(err.contains("Maxroll"));
    }

    /// Réseau : build complet de Ky depuis son lien Mobalytics (un .build par variante + runes).
    #[test]
    #[ignore]
    fn mobalytics_build_import() {
        let files = fetch_url("https://mobalytics.gg/poe-2/profile/pathofky/builds/cold-caster-chronomancer-leaguestart-to-endgame").unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        eprintln!("{names:?}");
        assert!(names.contains(&"Act 1 {1}.build") && names.contains(&KAMI_RUNES));
        let (steps, extras) = read_steps(files).unwrap();
        assert!(steps.len() >= 8 && extras.iter().any(|(n, _)| n == KAMI_RUNES) && extras.iter().any(|(n, _)| n == QUEST_CHOICES) && extras.iter().any(|(n, _)| n == SKILL_SETS)
                && extras.iter().any(|(n, _)| n == STEP_LINKS)
        );
    }
}
