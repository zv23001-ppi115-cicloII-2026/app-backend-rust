mod boundary;
mod config;
mod control;
mod entity;
mod error;

use std::{sync::Arc, time::Duration};

use boundary::{
    couchdb::{couch_client::CouchClient, couch_pedido_repo::CouchPedidoRepo},
    http::{handlers::AppState, router::crear_router},
    motor_rutas::{
        distancias::{HaversineUrbano, Osrm, ProveedorDistancias},
        optimizador_2opt::Optimizador2Opt,
    },
};
use control::{metricas::Metricas, pedido_control::PedidoControl, ruta_control::RutaControl};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg = config::Config::desde_entorno();
    let couch = CouchClient::new(&cfg)?;
    match couch.asegurar_db().await {
        Ok(()) => tracing::info!("CouchDB listo (base '{}')", cfg.couchdb_db),
        Err(e) => {
            tracing::warn!("No se pudo inicializar CouchDB: {e}. La API arrancará de todos modos.")
        }
    }
    if let Some(peer_url) = cfg.couchdb_replication_peer_url.as_deref() {
        match couch
            .configurar_replicacion(
                peer_url,
                cfg.couchdb_replication_peer_user.as_deref(),
                cfg.couchdb_replication_peer_password.as_deref(),
            )
            .await
        {
            Ok(()) => tracing::info!(
                "Replicación continua unidireccional a la base de respaldo configurada"
            ),
            Err(e) => tracing::warn!("No se pudo configurar la replicación CouchDB: {e}"),
        }
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

    let distancias: Arc<dyn ProveedorDistancias> = match &cfg.osrm_url {
        Some(url) => Arc::new(Osrm::new(url, cfg.osrm_timeout_ms, cfg.factor_circuito)),
        None => Arc::new(HaversineUrbano::new(cfg.factor_circuito)),
    };

    let optimizador = Arc::new(Optimizador2Opt::new(distancias));

    let estado = AppState {
        pedidos: Arc::new(PedidoControl::new(Arc::new(CouchPedidoRepo::new(
            couch.clone(),
        )))),
        rutas: Arc::new(RutaControl::new(optimizador)),
        metricas,
        couch,
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