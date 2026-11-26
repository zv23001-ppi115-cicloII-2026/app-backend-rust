//! Datos de entrada y salida del motor de rutas (10.1.2).
use serde::{Deserialize, Serialize};

use super::pedido::Coordenada;

#[derive(Debug, Clone, Deserialize)]
pub struct Parada {
    pub pedido_id: String,
    pub ubicacion: Coordenada,
    #[serde(default)]
    pub titulo_parada: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SolicitudRuta {
    pub repartidor_id: String,
    pub origen: Coordenada,
    pub paradas: Vec<Parada>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RutaOptimizada {
    pub repartidor_id: String,
    pub orden: Vec<String>,
    pub distancia_km: f64,
    pub algoritmo: String,
    pub tiempo_calculo_ms: f64,
}