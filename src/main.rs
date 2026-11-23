mod boundary;
mod config;
mod control;
mod entity;
mod error;

use std::{sync::Arc, time::Duration};

use boundary::{
    couchdb::{couch_client::CouchClient, couch_pedido_repo::CouchPedidoRepo},
    http::{handlers::AppState, router::crear_router},
    motor_rutas::optimizador_placeholder::OptimizadorPlaceholder,
};
use control::{metricas::Metricas, pedido_control::PedidoControl, ruta_control::RutaControl};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cfg = config::Config::desde_entorno();
    let couch = CouchClient::new(&cfg)?;
    match couch.asegurar_db().await {
        Ok(()) => tracing::info!("CouchDB listo (base '{}')", cfg.couchdb_db),
        Err(e) => tracing::warn!("No se pudo inicializar CouchDB: {e}. La API arrancará de todos modos."),
    }

    let metricas = Arc::new(Metricas::new());
    {
        let m = metricas.clone();
        tokio::spawn(async move {
            loop {
                m.refrescar_proceso();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    let estado = AppState {
        pedidos: Arc::new(PedidoControl::new(Arc::new(CouchPedidoRepo::new(couch)))),
        rutas: Arc::new(RutaControl::new(Arc::new(OptimizadorPlaceholder))),
        metricas,
    };

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!("API escuchando en {}", cfg.bind_addr);
    axum::serve(listener, crear_router(estado))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
