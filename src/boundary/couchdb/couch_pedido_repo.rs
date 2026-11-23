//! Implementación del puerto `PedidoRepository` sobre CouchDB.
use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::couch_client::CouchClient;
use crate::entity::pedido::Pedido;
use crate::control::puertos::{FiltroPedidos, PedidoRepository, ResultadoDoc};
use crate::error::AppError;

pub struct CouchPedidoRepo {
    couch: CouchClient,
}

impl CouchPedidoRepo {
    pub fn new(couch: CouchClient) -> Self {
        Self { couch }
    }
}

#[async_trait]
impl PedidoRepository for CouchPedidoRepo {
    async fn crear(&self, mut pedido: Pedido) -> Result<Pedido, AppError> {
        pedido.rev = None;
        let rev = self.couch.guardar(&pedido.id, &pedido).await?;
        pedido.rev = Some(rev);
        Ok(pedido)
    }

    async fn obtener(&self, id: &str) -> Result<Pedido, AppError> {
        self.couch.leer::<Pedido>(id).await
    }

    async fn listar(&self, filtro: FiltroPedidos) -> Result<Vec<Pedido>, AppError> {
        let mut sel = Map::new();
        sel.insert("tipo".into(), json!("pedido"));
        if let Some(e) = filtro.estado {
            sel.insert("estado".into(), json!(e.como_str()));
        }
        if let Some(r) = filtro.repartidor_id {
            sel.insert("repartidor_id".into(), json!(r));
        }
        self.couch.buscar::<Pedido>(Value::Object(sel), filtro.limite).await
    }

    async fn actualizar(&self, mut pedido: Pedido) -> Result<Pedido, AppError> {
        let rev = self.couch.guardar(&pedido.id, &pedido).await?;
        pedido.rev = Some(rev);
        Ok(pedido)
    }

    async fn crear_lote(&self, pedidos: Vec<Pedido>) -> Result<Vec<ResultadoDoc>, AppError> {
        let resultados = self.couch.guardar_lote(&pedidos).await?;
        Ok(resultados
            .into_iter()
            .map(|r| {
                let ok = r.ok.unwrap_or(false);
                let razon = if ok {
                    None
                } else if r.error.as_deref() == Some("conflict") {
                    Some("ya existe un documento con ese id".to_string())
                } else {
                    Some(r.reason.or(r.error).unwrap_or_else(|| "error desconocido".into()))
                };
                ResultadoDoc { id: r.id.unwrap_or_default(), ok, razon }
            })
            .collect())
    }
}
