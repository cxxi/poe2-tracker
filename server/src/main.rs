//! Serveur de poe2-tracker : les profils publics des personnages, lus par une route publique.
//! Réglé par variables d'environnement : `DATABASE_URL` (MySQL ou MariaDB), `LISTEN` (adresse
//! d'écoute, 127.0.0.1:8787 par défaut : un serveur web devant fait le HTTPS), `RUST_LOG`.

mod api;

use sqlx::mysql::MySqlPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL manquant")?;
    let listen = std::env::var("LISTEN").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let db = MySqlPoolOptions::new().max_connections(5).connect(&url).await?;
    sqlx::migrate!().run(&db).await?;
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("à l'écoute sur {listen}");
    axum::serve(listener, api::router(db)).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

/// Arrêt propre sur Ctrl+C ou SIGTERM (systemd).
async fn shutdown() {
    let term = async {
        #[cfg(unix)]
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
            return;
        }
        std::future::pending::<()>().await;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term => {}
    }
}
