//! Puertos (traits) que `control` necesita del exterior. Las implementaciones concretas viven en
//! `boundary` (CouchDB: P3; motor de rutas: P2). `control` nunca depende de `boundary`.
use async_trait::async_trait;
use serde::Serialize;

use crate::entity::pedido::{EstadoPedido, Pedido};
use crate::entity::ruta::{RutaOptimizada, SolicitudRuta};
use crate::error::AppError;

#[derive(Debug, Clone, Default)]
pub struct FiltroPedidos {
    pub estado: Option<EstadoPedido>,
    pub repartidor_id: Option<String>,
    pub limite: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResultadoDoc {
    pub id: String,
    pub ok: bool,
    pub razon: Option<String>,
}

#[async_trait]
pub trait PedidoRepository: Send + Sync {
    async fn crear(&self, pedido: Pedido) -> Result<Pedido, AppError>;
    async fn obtener(&self, id: &str) -> Result<Pedido, AppError>;
    async fn listar(&self, filtro: FiltroPedidos) -> Result<Vec<Pedido>, AppError>;
    async fn actualizar(&self, pedido: Pedido) -> Result<Pedido, AppError>;
    async fn crear_lote(&self, pedidos: Vec<Pedido>) -> Result<Vec<ResultadoDoc>, AppError>;
}

/// Firma que P2 debe implementar. Es síncrona y CPU-bound: `RutaControl` la ejecuta en
/// `spawn_blocking` para no bloquear el runtime asíncrono.
pub trait OptimizadorRutas: Send + Sync {
    fn optimizar(&self, solicitud: &SolicitudRuta) -> Result<RutaOptimizada, AppError>;
}
