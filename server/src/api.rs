//! Routes de l'API.
//!
//! - `POST /v1/installs` : nouvelle installation de l'app, son jeton secret en retour.
//! - `PUT /v1/me/profiles/{personnage}` (jeton) : envoie le profil public d'un personnage ;
//!   son identifiant public en retour, stable pour ce personnage.
//! - `DELETE /v1/me/profiles/{personnage}` (jeton) : profil retiré (profil public décoché).
//! - `GET /v1/profiles/{id}` : le profil, en JSON ; `{id}.md` : l'export en Markdown.
//! - `GET /v1/map-bosses` : boss connus des maps (code → nom).
//! - `PUT /v1/me/map-bosses/{code}` (jeton) : boss d'une map lu à l'écran.
//! - `POST /v1/me/reports` (jeton) : rapport d'erreurs de l'app (les 50 derniers gardés).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use poe2_shared::{ApiError, InstallCreated, MapBoss, MapBosses, PROFILE_FORMAT, Profile, ProfileSaved, PublicProfile, Report};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::MySqlPool;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

/// Taille maximale d'une requête : un export fait quelques dizaines de Ko.
const MAX_BODY: usize = 1024 * 1024;

pub fn router(db: MySqlPool) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/installs", post(create_install))
        .route("/v1/me/profiles/{character}", put(save_profile).delete(delete_profile))
        .route("/v1/profiles/{id}", get(public_profile))
        .route("/v1/map-bosses", get(map_bosses))
        .route("/v1/me/map-bosses/{code}", put(save_map_boss))
        .route("/v1/me/reports", post(save_report))
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
        .layer(TraceLayer::new_for_http())
        .with_state(db)
}

pub enum Error {
    Unauthorized,
    NotFound,
    BadRequest(String),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::Db(e)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, error) = match self {
            Error::Unauthorized => (StatusCode::UNAUTHORIZED, "jeton manquant ou inconnu".to_string()),
            Error::NotFound => (StatusCode::NOT_FOUND, "profil introuvable".to_string()),
            Error::BadRequest(e) => (StatusCode::BAD_REQUEST, e),
            Error::Db(e) => {
                tracing::error!("base de données : {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "erreur du serveur".to_string())
            }
        };
        (status, Json(ApiError { error })).into_response()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `n` octets aléatoires en base32 minuscule (sans remplissage) : jetons et identifiants.
fn random_id(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::rng().fill_bytes(&mut bytes);
    base32(&bytes)
}

fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Installation qui porte le jeton de la requête (`Authorization: Bearer …`).
async fn install_of(db: &MySqlPool, headers: &HeaderMap) -> Result<u64, Error> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(Error::Unauthorized)?;
    let id: Option<u64> = sqlx::query_scalar("SELECT id FROM installs WHERE token_hash = ?").bind(hash(token.trim())).fetch_optional(db).await?;
    let id = id.ok_or(Error::Unauthorized)?;
    sqlx::query("UPDATE installs SET last_seen = ? WHERE id = ?").bind(now()).bind(id).execute(db).await?;
    Ok(id)
}

async fn create_install(State(db): State<MySqlPool>) -> Result<(StatusCode, Json<InstallCreated>), Error> {
    let token = random_id(32);
    let t = now();
    sqlx::query("INSERT INTO installs (token_hash, created_at, last_seen) VALUES (?, ?, ?)").bind(hash(&token)).bind(t).bind(t).execute(&db).await?;
    Ok((StatusCode::CREATED, Json(InstallCreated { token })))
}

async fn save_profile(State(db): State<MySqlPool>, headers: HeaderMap, Path(character): Path<String>, Json(profile): Json<Profile>) -> Result<Json<ProfileSaved>, Error> {
    let install = install_of(&db, &headers).await?;
    if character.is_empty() || character.chars().count() > 64 || profile.character.name != character {
        return Err(Error::BadRequest("nom de personnage invalide".into()));
    }
    if profile.format != PROFILE_FORMAT {
        return Err(Error::BadRequest(format!("format de profil {} non géré (attendu : {PROFILE_FORMAT})", profile.format)));
    }
    let data = serde_json::to_string(&profile).map_err(|e| Error::BadRequest(e.to_string()))?;
    let t = now();
    let existing: Option<String> = sqlx::query_scalar("SELECT id FROM profiles WHERE install_id = ? AND character_name = ?").bind(install).bind(&character).fetch_optional(&db).await?;
    let id = match existing {
        Some(id) => {
            sqlx::query("UPDATE profiles SET data = ?, updated_at = ? WHERE id = ?").bind(&data).bind(t).bind(&id).execute(&db).await?;
            id
        }
        None => {
            let id = random_id(16);
            sqlx::query("INSERT INTO profiles (id, install_id, character_name, data, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(install)
                .bind(&character)
                .bind(&data)
                .bind(t)
                .bind(t)
                .execute(&db)
                .await?;
            id
        }
    };
    Ok(Json(ProfileSaved { id }))
}

async fn delete_profile(State(db): State<MySqlPool>, headers: HeaderMap, Path(character): Path<String>) -> Result<StatusCode, Error> {
    let install = install_of(&db, &headers).await?;
    sqlx::query("DELETE FROM profiles WHERE install_id = ? AND character_name = ?").bind(install).bind(&character).execute(&db).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn public_profile(State(db): State<MySqlPool>, Path(id): Path<String>) -> Result<Response, Error> {
    let (id, markdown) = match id.strip_suffix(".md") {
        Some(id) => (id.to_string(), true),
        None => (id, false),
    };
    if id.len() != 26 || !id.bytes().all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b)) {
        return Err(Error::NotFound);
    }
    let row: Option<(String, i64)> = sqlx::query_as("SELECT data, updated_at FROM profiles WHERE id = ?").bind(&id).fetch_optional(&db).await?;
    let (data, updated_at) = row.ok_or(Error::NotFound)?;
    let profile: Profile = serde_json::from_str(&data).map_err(|e| Error::Db(sqlx::Error::Decode(Box::new(e))))?;
    if markdown {
        return Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], profile.markdown).into_response());
    }
    Ok(Json(PublicProfile { id, updated_at, profile }).into_response())
}

/// Boss de chaque map : le nom le plus signalé (à égalité, le plus récent).
async fn map_bosses(State(db): State<MySqlPool>) -> Result<Json<MapBosses>, Error> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT code, boss FROM map_bosses GROUP BY code, boss ORDER BY code, COUNT(*) DESC, MAX(updated_at) DESC").fetch_all(&db).await?;
    let mut out = MapBosses::new();
    for (code, boss) in rows {
        out.entry(code).or_insert(boss);
    }
    Ok(Json(out))
}

async fn save_map_boss(State(db): State<MySqlPool>, headers: HeaderMap, Path(code): Path<String>, Json(b): Json<MapBoss>) -> Result<StatusCode, Error> {
    let install = install_of(&db, &headers).await?;
    if code.is_empty() || code.len() > 80 || !code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
        return Err(Error::BadRequest("code de map invalide".into()));
    }
    let boss = b.boss.trim();
    if boss.is_empty() || boss.chars().count() > 120 {
        return Err(Error::BadRequest("nom de boss invalide".into()));
    }
    sqlx::query("INSERT INTO map_bosses (install_id, code, boss, updated_at) VALUES (?, ?, ?, ?) ON DUPLICATE KEY UPDATE boss = VALUES(boss), updated_at = VALUES(updated_at)")
        .bind(install)
        .bind(&code)
        .bind(boss)
        .bind(now())
        .execute(&db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Rapports gardés par installation : les plus anciens partent.
const REPORTS_KEPT: i64 = 50;

async fn save_report(State(db): State<MySqlPool>, headers: HeaderMap, Json(r): Json<Report>) -> Result<StatusCode, Error> {
    let install = install_of(&db, &headers).await?;
    if r.entries.is_empty() || r.entries.len() > 500 || r.app_version.len() > 32 || r.os.len() > 32 || r.character.as_ref().is_some_and(|c| c.chars().count() > 64) {
        return Err(Error::BadRequest("rapport invalide".into()));
    }
    let entries = serde_json::to_string(&r.entries).map_err(|e| Error::BadRequest(e.to_string()))?;
    sqlx::query("INSERT INTO reports (install_id, created_at, app_version, os, character_name, entries, log_tail) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(install)
        .bind(now())
        .bind(&r.app_version)
        .bind(&r.os)
        .bind(&r.character)
        .bind(&entries)
        .bind(&r.log_tail)
        .execute(&db)
        .await?;
    let old: Option<u64> = sqlx::query_scalar("SELECT id FROM reports WHERE install_id = ? ORDER BY id DESC LIMIT 1 OFFSET ?").bind(install).bind(REPORTS_KEPT - 1).fetch_optional(&db).await?;
    if let Some(old) = old {
        sqlx::query("DELETE FROM reports WHERE install_id = ? AND id < ?").bind(install).bind(old).execute(&db).await?;
    }
    Ok(StatusCode::CREATED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[test]
    fn base32_ids() {
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
        assert_eq!(random_id(16).len(), 26);
        assert_eq!(random_id(32).len(), 52);
    }

    async fn call(app: &Router, req: Request<Body>) -> (StatusCode, String) {
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// Parcours complet sur une vraie base (vidée au départ) :
    /// TEST_DATABASE_URL=mysql://… cargo test -p poe2-server -- --ignored
    #[tokio::test]
    #[ignore]
    async fn profile_round_trip() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else { return };
        let db = MySqlPool::connect(&url).await.unwrap();
        sqlx::query("DROP TABLE IF EXISTS reports, map_bosses, profiles, installs, _sqlx_migrations").execute(&db).await.unwrap();
        sqlx::migrate!().run(&db).await.unwrap();
        let app = router(db);

        let (status, body) = call(&app, Request::post("/v1/installs").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::CREATED);
        let token = serde_json::from_str::<InstallCreated>(&body).unwrap().token;

        let mut profile = Profile { format: PROFILE_FORMAT, lang: "fr".into(), markdown: "# Juliana".into(), ..Default::default() };
        profile.character.name = "Juliana".into();
        let put = |p: &Profile, token: &str| {
            Request::put("/v1/me/profiles/Juliana")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(p).unwrap()))
                .unwrap()
        };
        let (status, body) = call(&app, put(&profile, &token)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let id = serde_json::from_str::<ProfileSaved>(&body).unwrap().id;
        // Second envoi : même identifiant, contenu remplacé.
        profile.markdown = "# Juliana v2".into();
        let (status, body) = call(&app, put(&profile, &token)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(serde_json::from_str::<ProfileSaved>(&body).unwrap().id, id);
        // Mauvais jeton : refusé.
        assert_eq!(call(&app, put(&profile, "faux")).await.0, StatusCode::UNAUTHORIZED);

        let (status, body) = call(&app, Request::get(format!("/v1/profiles/{id}")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(serde_json::from_str::<PublicProfile>(&body).unwrap().profile, profile);
        let (_, body) = call(&app, Request::get(format!("/v1/profiles/{id}.md")).body(Body::empty()).unwrap()).await;
        assert_eq!(body, "# Juliana v2");

        // Boss des maps : le plus signalé.
        let boss = |code: &str, name: &str, token: &str| {
            Request::put(format!("/v1/me/map-bosses/{code}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&MapBoss { boss: name.into() }).unwrap()))
                .unwrap()
        };
        let (_, body) = call(&app, Request::post("/v1/installs").body(Body::empty()).unwrap()).await;
        let other = serde_json::from_str::<InstallCreated>(&body).unwrap().token;
        let (_, body) = call(&app, Request::post("/v1/installs").body(Body::empty()).unwrap()).await;
        let third = serde_json::from_str::<InstallCreated>(&body).unwrap().token;
        assert_eq!(call(&app, boss("MapSlick", "Vorrik, The Infernal Engineer", &token)).await.0, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, boss("MapSlick", "Vorrik, The Infernal Engineer", &other)).await.0, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, boss("MapSlick", "Mist Raven", &third)).await.0, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, boss("Map%20Slick", "Mist Raven", &token)).await.0, StatusCode::BAD_REQUEST);
        let (_, body) = call(&app, Request::get("/v1/map-bosses").body(Body::empty()).unwrap()).await;
        assert_eq!(serde_json::from_str::<MapBosses>(&body).unwrap().get("MapSlick").map(String::as_str), Some("Vorrik, The Infernal Engineer"));
        // Rapports : seulement les 50 derniers.
        let report = Report { app_version: "0.9.0".into(), os: "linux".into(), character: Some("Juliana".into()), entries: vec![poe2_shared::ReportEntry { at: 1, level: "ERROR".into(), message: "boum".into() }], log_tail: "…".into() };
        for _ in 0..52 {
            let req = Request::post("/v1/me/reports").header(header::AUTHORIZATION, format!("Bearer {token}")).header(header::CONTENT_TYPE, "application/json").body(Body::from(serde_json::to_string(&report).unwrap())).unwrap();
            assert_eq!(call(&app, req).await.0, StatusCode::CREATED);
        }
        let db = MySqlPool::connect(&url).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reports").fetch_one(&db).await.unwrap();
        assert_eq!(n, REPORTS_KEPT);

        let del = Request::delete("/v1/me/profiles/Juliana").header(header::AUTHORIZATION, format!("Bearer {token}")).body(Body::empty()).unwrap();
        assert_eq!(call(&app, del).await.0, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, Request::get(format!("/v1/profiles/{id}")).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
    }
}
