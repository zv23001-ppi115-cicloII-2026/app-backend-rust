//! Definición de rutas y capas (medición, trazas, CORS).
use axum::{
    middleware,
    routing::{get, patch, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use super::handlers::{self, AppState};
use super::middleware::medir;

pub fn crear_router(estado: AppState) -> Router {
    let metricas = estado.metricas.clone();
    Router::new()
        .route("/health", get(handlers::salud))
        .route("/api/v1/pedidos", post(handlers::crear_pedido).get(handlers::listar_pedidos))
        .route("/api/v1/pedidos/lote", post(handlers::crear_lote))
        .route("/api/v1/pedidos/:id", get(handlers::obtener_pedido))
        .route("/api/v1/pedidos/:id/estado", patch(handlers::cambiar_estado))
        .route("/api/v1/rutas/optimizar", post(handlers::optimizar_ruta))
        .route("/api/v1/metricas", get(handlers::metricas))
        .route("/api/v1/resumen-operativo", get(handlers::resumen_operativo))
        .layer(middleware::from_fn_with_state(metricas, medir))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // acotar orígenes fuera del entorno de laboratorio
        .with_state(estado)
}