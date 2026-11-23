//! Controladores HTTP: traducen petición/respuesta y delegan en los servicios.
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::control::{
    metricas::{Metricas, Snapshot},
    pedido_control::{PedidoControl, ResultadoLote},
    puertos::FiltroPedidos,
    ruta_control::RutaControl,
};
use crate::entity::pedido::{EstadoPedido, NuevoPedido, Pedido};
use crate::entity::ruta::{RutaOptimizada, SolicitudRuta};
use crate::error::AppError;

#[derive(Clone)]
pub struct AppState {
    pub pedidos: Arc<PedidoControl>,
    pub rutas: Arc<RutaControl>,
    pub metricas: Arc<Metricas>,
}

#[derive(Debug, Deserialize)]
pub struct ListarQuery {
    pub estado: Option<EstadoPedido>,
    pub repartidor_id: Option<String>,
    pub limite: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CambioEstado {
    pub estado: EstadoPedido,
    pub repartidor_id: Option<String>,
    pub entregado_en: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct LotePedidos {
    pub pedidos: Vec<NuevoPedido>,
}

pub async fn salud() -> Json<Value> {
    Json(json!({ "estado": "ok", "servicio": "reparto-backend", "version": env!("CARGO_PKG_VERSION") }))
}

pub async fn crear_pedido(
    State(s): State<AppState>,
    Json(entrada): Json<NuevoPedido>,
) -> Result<(StatusCode, Json<Pedido>), AppError> {
    let p = s.pedidos.crear(entrada).await?;
    s.metricas.contar_docs(1);
    Ok((StatusCode::CREATED, Json(p)))
}

pub async fn crear_lote(
    State(s): State<AppState>,
    Json(lote): Json<LotePedidos>,
) -> Result<Json<ResultadoLote>, AppError> {
    let r = s.pedidos.crear_lote(lote.pedidos).await?;
    s.metricas.contar_docs(r.aceptados as u64);
    Ok(Json(r))
}

pub async fn listar_pedidos(
    State(s): State<AppState>,
    Query(q): Query<ListarQuery>,
) -> Result<Json<Vec<Pedido>>, AppError> {
    let filtro = FiltroPedidos { estado: q.estado, repartidor_id: q.repartidor_id, limite: q.limite.unwrap_or(100) };
    Ok(Json(s.pedidos.listar(filtro).await?))
}

pub async fn obtener_pedido(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Pedido>, AppError> {
    Ok(Json(s.pedidos.obtener(&id).await?))
}

pub async fn cambiar_estado(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(c): Json<CambioEstado>,
) -> Result<Json<Pedido>, AppError> {
    let p = s.pedidos.cambiar_estado(&id, c.estado, c.repartidor_id, c.entregado_en).await?;
    s.metricas.contar_docs(1);
    Ok(Json(p))
}

pub async fn optimizar_ruta(
    State(s): State<AppState>,
    Json(sol): Json<SolicitudRuta>,
) -> Result<Json<RutaOptimizada>, AppError> {
    Ok(Json(s.rutas.optimizar(sol).await?))
}

pub async fn metricas(State(s): State<AppState>) -> Json<Snapshot> {
    Json(s.metricas.snapshot())
}
