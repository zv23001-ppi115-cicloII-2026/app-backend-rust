//! Control: casos de uso de pedidos. Orquesta las entidades y el puerto de persistencia.
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::entity::pedido::{id_valido, EstadoPedido, NuevoPedido, Pedido};
use super::puertos::{FiltroPedidos, PedidoRepository, ResultadoDoc};
use crate::error::AppError;

const MAX_LOTE: usize = 500;
const MAX_LISTADO: u32 = 500;

#[derive(Debug, Serialize)]
pub struct ResultadoLote {
    pub aceptados: usize,
    pub rechazados: Vec<ResultadoDoc>,
}

pub struct PedidoControl {
    repo: Arc<dyn PedidoRepository>,
}

impl PedidoControl {
    pub fn new(repo: Arc<dyn PedidoRepository>) -> Self {
        Self { repo }
    }

    pub async fn crear(&self, entrada: NuevoPedido) -> Result<Pedido, AppError> {
        let pedido = Pedido::nuevo(entrada)?;
        self.repo.crear(pedido).await
    }

    pub async fn obtener(&self, id: &str) -> Result<Pedido, AppError> {
        if !id_valido(id) {
            return Err(AppError::Validacion("id con formato inválido".into()));
        }
        self.repo.obtener(id).await
    }

    pub async fn listar(&self, mut filtro: FiltroPedidos) -> Result<Vec<Pedido>, AppError> {
        if filtro.limite == 0 || filtro.limite > MAX_LISTADO {
            filtro.limite = 100;
        }
        self.repo.listar(filtro).await
    }

    pub async fn cambiar_estado(
        &self,
        id: &str,
        nuevo: EstadoPedido,
        repartidor_id: Option<String>,
        entregado_en: Option<DateTime<Utc>>,
    ) -> Result<Pedido, AppError> {
        let mut pedido = self.obtener(id).await?;
        if pedido.estado == nuevo {
            return Ok(pedido); // idempotente ante reintentos del cliente offline
        }
        if !pedido.estado.puede_pasar_a(nuevo) {
            return Err(AppError::Conflicto(format!(
                "Transición no permitida: {} -> {}",
                pedido.estado.como_str(),
                nuevo.como_str()
            )));
        }
        if nuevo == EstadoPedido::Asignado {
            let r = repartidor_id
                .filter(|r| id_valido(r))
                .ok_or_else(|| AppError::Validacion("repartidor_id es obligatorio al asignar".into()))?;
            pedido.repartidor_id = Some(r);
        }
        let ahora = Utc::now();
        if nuevo == EstadoPedido::Entregado {
            pedido.entregado_en = Some(entregado_en.unwrap_or(ahora));
        }
        pedido.estado = nuevo;
        pedido.actualizado_en = ahora;
        self.repo.actualizar(pedido).await
    }

    /// Ingesta masiva (p. ej. cola acumulada offline). Valida cada documento y persiste en una sola operación.
    pub async fn crear_lote(&self, entradas: Vec<NuevoPedido>) -> Result<ResultadoLote, AppError> {
        if entradas.is_empty() || entradas.len() > MAX_LOTE {
            return Err(AppError::Validacion(format!("El lote debe tener entre 1 y {MAX_LOTE} pedidos")));
        }
        let mut validos = Vec::with_capacity(entradas.len());
        let mut rechazados = Vec::new();
        for (i, e) in entradas.into_iter().enumerate() {
            let id_ref = e.id.clone().unwrap_or_else(|| format!("#{i}"));
            match Pedido::nuevo(e) {
                Ok(p) => validos.push(p),
                Err(err) => rechazados.push(ResultadoDoc { id: id_ref, ok: false, razon: Some(err.to_string()) }),
            }
        }
        let mut aceptados = 0;
        if !validos.is_empty() {
            for r in self.repo.crear_lote(validos).await? {
                if r.ok {
                    aceptados += 1;
                } else {
                    rechazados.push(r);
                }
            }
        }
        Ok(ResultadoLote { aceptados, rechazados })
    }
}
