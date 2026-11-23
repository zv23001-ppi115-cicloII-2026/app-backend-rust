//! Control: caso de uso de cálculo de rutas: valida la solicitud y delega en el motor de P2.
use std::sync::Arc;

use crate::entity::pedido::id_valido;
use crate::entity::ruta::{RutaOptimizada, SolicitudRuta};
use super::puertos::OptimizadorRutas;
use crate::error::AppError;

const MAX_PARADAS: usize = 50;

pub struct RutaControl {
    optimizador: Arc<dyn OptimizadorRutas>,
}

impl RutaControl {
    pub fn new(optimizador: Arc<dyn OptimizadorRutas>) -> Self {
        Self { optimizador }
    }

    pub async fn optimizar(&self, solicitud: SolicitudRuta) -> Result<RutaOptimizada, AppError> {
        if !id_valido(&solicitud.repartidor_id) {
            return Err(AppError::Validacion("repartidor_id inválido".into()));
        }
        if solicitud.paradas.is_empty() || solicitud.paradas.len() > MAX_PARADAS {
            return Err(AppError::Validacion(format!("Se requieren entre 1 y {MAX_PARADAS} paradas")));
        }
        solicitud.origen.validar()?;
        for p in &solicitud.paradas {
            p.ubicacion.validar()?;
        }
        let opt = Arc::clone(&self.optimizador);
        // Cómputo intensivo en CPU: se aísla del runtime asíncrono.
        tokio::task::spawn_blocking(move || opt.optimizar(&solicitud))
            .await
            .map_err(|e| AppError::Interno(format!("fallo en tarea de cálculo: {e}")))?
    }
}
