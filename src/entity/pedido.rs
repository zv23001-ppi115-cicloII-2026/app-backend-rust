//! Entidad Pedido y reglas de negocio puras (sin dependencias de HTTP ni CouchDB).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoPedido {
    Pendiente,
    Asignado,
    EnRuta,
    Entregado,
    Cancelado,
}

impl EstadoPedido {
    pub fn como_str(self) -> &'static str {
        match self {
            EstadoPedido::Pendiente => "pendiente",
            EstadoPedido::Asignado => "asignado",
            EstadoPedido::EnRuta => "en_ruta",
            EstadoPedido::Entregado => "entregado",
            EstadoPedido::Cancelado => "cancelado",
        }
    }

    /// Máquina de estados del pedido.
    pub fn puede_pasar_a(self, nuevo: EstadoPedido) -> bool {
        use EstadoPedido::*;
        matches!(
            (self, nuevo),
            (Pendiente, Asignado)
                | (Pendiente, Cancelado)
                | (Asignado, EnRuta)
                | (Asignado, Pendiente)
                | (Asignado, Cancelado)
                | (EnRuta, Entregado)
                | (EnRuta, Cancelado)
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Coordenada {
    pub lat: f64,
    pub lng: f64,
}

impl Coordenada {
    pub fn validar(&self) -> Result<(), AppError> {
        if !(-90.0..=90.0).contains(&self.lat) || !(-180.0..=180.0).contains(&self.lng) {
            return Err(AppError::Validacion("Coordenada fuera de rango".into()));
        }
        Ok(())
    }
}

/// Identificadores permitidos: alfanuméricos, '-', '_' y ':'; no pueden iniciar con '_' (reservado en CouchDB).
pub fn id_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('_')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
}

fn metodo_pago_default() -> String {
    "efectivo".to_string()
}

/// Documento persistido en CouchDB (campo `tipo` = "pedido").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pedido {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    pub tipo: String,
    pub cliente: String,
    #[serde(default)]
    pub titulo_parada: Option<String>,
    pub direccion: String,
    #[serde(default)]
    pub referencia_visual: Option<String>,
    pub ubicacion: Coordenada,
    pub total: f64,
    #[serde(default)]
    pub subtotal_productos: Option<f64>,
    #[serde(default)]
    pub costo_envio: Option<f64>,
    #[serde(default = "metodo_pago_default")]
    pub metodo_pago: String, // efectivo, transferencia, pos, pagado
    #[serde(default)]
    pub paga_con: Option<f64>,
    #[serde(default)]
    pub orden_en_ruta: Option<u32>,
    #[serde(default)]
    pub motivo_incidencia: Option<String>,
    pub estado: EstadoPedido,
    #[serde(default)]
    pub repartidor_id: Option<String>,
    pub creado_en: DateTime<Utc>,
    pub actualizado_en: DateTime<Utc>,
    #[serde(default)]
    pub inicio_ruta_en: Option<DateTime<Utc>>,
    #[serde(default)]
    pub entregado_en: Option<DateTime<Utc>>,
}

/// Datos de entrada para crear un pedido. `id` opcional: el cliente offline lo genera para idempotencia.
#[derive(Debug, Clone, Deserialize)]
pub struct NuevoPedido {
    pub id: Option<String>,
    pub cliente: String,
    #[serde(default)]
    pub titulo_parada: Option<String>,
    pub direccion: String,
    #[serde(default)]
    pub referencia_visual: Option<String>,
    pub ubicacion: Coordenada,
    pub total: f64,
    #[serde(default)]
    pub subtotal_productos: Option<f64>,
    #[serde(default)]
    pub costo_envio: Option<f64>,
    #[serde(default)]
    pub metodo_pago: Option<String>,
    #[serde(default)]
    pub paga_con: Option<f64>,
    pub creado_en: Option<DateTime<Utc>>,
}

impl Pedido {
    pub fn nuevo(n: NuevoPedido) -> Result<Pedido, AppError> {
        if n.cliente.trim().is_empty() || n.direccion.trim().is_empty() {
            return Err(AppError::Validacion("cliente y direccion son obligatorios".into()));
        }
        if !n.total.is_finite() || n.total < 0.0 {
            return Err(AppError::Validacion("total debe ser un número >= 0".into()));
        }
        n.ubicacion.validar()?;
        let id = match n.id {
            Some(id) if id_valido(&id) => id,
            Some(_) => return Err(AppError::Validacion("id con formato inválido".into())),
            None => format!("pedido:{}", uuid::Uuid::new_v4()),
        };
        let ahora = Utc::now();
        let cliente_limpio = n.cliente.trim().to_string();
        let titulo = n
            .titulo_parada
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| Some(format!("Entrega · {}", cliente_limpio)));

        let costo_envio = n.costo_envio.unwrap_or(0.0).max(0.0);
        let subtotal = n
            .subtotal_productos
            .unwrap_or_else(|| (n.total - costo_envio).max(0.0));

        Ok(Pedido {
            id,
            rev: None,
            tipo: "pedido".into(),
            cliente: cliente_limpio,
            titulo_parada: titulo,
            direccion: n.direccion.trim().to_string(),
            referencia_visual: n
                .referencia_visual
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty()),
            ubicacion: n.ubicacion,
            total: n.total,
            subtotal_productos: Some(subtotal),
            costo_envio: Some(costo_envio),
            metodo_pago: n
                .metodo_pago
                .map(|m| m.trim().to_lowercase())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(metodo_pago_default),
            paga_con: n.paga_con,
            orden_en_ruta: None,
            motivo_incidencia: None,
            estado: EstadoPedido::Pendiente,
            repartidor_id: None,
            creado_en: n.creado_en.unwrap_or(ahora),
            actualizado_en: ahora,
            inicio_ruta_en: None,
            entregado_en: None,
        })
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn transiciones_validas_e_invalidas() {
        assert!(EstadoPedido::Pendiente.puede_pasar_a(EstadoPedido::Asignado));
        assert!(EstadoPedido::EnRuta.puede_pasar_a(EstadoPedido::Entregado));
        assert!(!EstadoPedido::Entregado.puede_pasar_a(EstadoPedido::Pendiente));
        assert!(!EstadoPedido::Pendiente.puede_pasar_a(EstadoPedido::Entregado));
    }

    #[test]
    fn rechaza_coordenadas_fuera_de_rango() {
        let c = Coordenada { lat: 95.0, lng: 0.0 };
        assert!(c.validar().is_err());
    }

    #[test]
    fn valida_ids() {
        assert!(id_valido("pedido:abc-123"));
        assert!(!id_valido("_design/x"));
        assert!(!id_valido(""));
    }
}