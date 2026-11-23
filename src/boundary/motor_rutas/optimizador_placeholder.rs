//! IMPLEMENTACIÓN TEMPORAL del motor de rutas: conserva el orden recibido y calcula la distancia
//! con la fórmula de Haversine. Debe sustituirse por el algoritmo de P2 (10.1.2) implementando
//! el trait `OptimizadorRutas`; el resto de la API no requiere cambios.
use std::time::Instant;

use crate::control::puertos::OptimizadorRutas;
use crate::entity::pedido::Coordenada;
use crate::entity::ruta::{RutaOptimizada, SolicitudRuta};
use crate::error::AppError;

pub struct OptimizadorPlaceholder;

fn haversine_km(a: &Coordenada, b: &Coordenada) -> f64 {
    let r = 6371.0;
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = (b.lat - a.lat).to_radians();
    let dlng = (b.lng - a.lng).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlng / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

impl OptimizadorRutas for OptimizadorPlaceholder {
    fn optimizar(&self, s: &SolicitudRuta) -> Result<RutaOptimizada, AppError> {
        let t = Instant::now();
        let mut previo = s.origen;
        let mut total = 0.0;
        for p in &s.paradas {
            total += haversine_km(&previo, &p.ubicacion);
            previo = p.ubicacion;
        }
        Ok(RutaOptimizada {
            repartidor_id: s.repartidor_id.clone(),
            orden: s.paradas.iter().map(|p| p.pedido_id.clone()).collect(),
            distancia_km: (total * 1000.0).round() / 1000.0,
            algoritmo: "placeholder-orden-recibido".into(),
            tiempo_calculo_ms: t.elapsed().as_secs_f64() * 1000.0,
        })
    }
}
