//! Fuentes de distancias para el motor de rutas (P2, 10.1.2).
//! `HaversineUrbano`: sin red, distancia en línea recta × factor de circuito.
//! `Osrm`: distancias por la red vial de OpenStreetMap (OSRM), con respaldo a Haversine.
use std::{sync::OnceLock, time::Duration};

use crate::entity::pedido::Coordenada;
use crate::error::AppError;

/// Matriz cuadrada en km: `m[i][j]` = distancia de ir del punto `i` al punto `j`.
pub type Matriz = Vec<Vec<f64>>;

pub trait ProveedorDistancias: Send + Sync {
    /// Devuelve la matriz y el nombre de la fuente realmente usada.
    fn matriz_km(&self, puntos: &[Coordenada]) -> Result<(Matriz, &'static str), AppError>;
}

pub fn haversine_km(a: &Coordenada, b: &Coordenada) -> f64 {
    let r = 6371.0;
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let dlat = (b.lat - a.lat).to_radians();
    let dlng = (b.lng - a.lng).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlng / 2.0).sin().powi(2);
    2.0 * r * h.sqrt().asin()
}

// ---------------------------------------------------------------- Haversine

pub struct HaversineUrbano {
    factor_circuito: f64,
}

impl HaversineUrbano {
    /// `factor_circuito` típico en trama urbana: 1.2 a 1.4.
    pub fn new(factor_circuito: f64) -> Self {
        Self { factor_circuito }
    }
}

impl ProveedorDistancias for HaversineUrbano {
    fn matriz_km(&self, puntos: &[Coordenada]) -> Result<(Matriz, &'static str), AppError> {
        let n = puntos.len();
        let mut m = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    m[i][j] = haversine_km(&puntos[i], &puntos[j]) * self.factor_circuito;
                }
            }
        }
        Ok((m, "haversine"))
    }
}

// --------------------------------------------------------------------- OSRM

pub struct Osrm {
    base_url: String,
    timeout: Duration,
    // El cliente bloqueante se crea de forma perezosa dentro del hilo de `spawn_blocking`
    // (crearlo en el contexto async de `main` provocaría un panic).
    cliente: OnceLock<Option<reqwest::blocking::Client>>,
    respaldo: HaversineUrbano,
}

impl Osrm {
    pub fn new(base_url: &str, timeout_ms: u64, factor_circuito: f64) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            timeout: Duration::from_millis(timeout_ms),
            cliente: OnceLock::new(),
            respaldo: HaversineUrbano::new(factor_circuito),
        }
    }

    fn consultar(&self, puntos: &[Coordenada]) -> Result<Matriz, String> {
        let cliente = self
            .cliente
            .get_or_init(|| reqwest::blocking::Client::builder().timeout(self.timeout).build().ok())
            .as_ref()
            .ok_or("no se pudo crear el cliente HTTP")?;

        // OSRM espera "lng,lat" separados por ';'
        let coords = puntos
            .iter()
            .map(|p| format!("{:.6},{:.6}", p.lng, p.lat))
            .collect::<Vec<_>>()
            .join(";");
        let url = format!("{}/table/v1/driving/{}?annotations=distance", self.base_url, coords);

        let v: serde_json::Value = cliente
            .get(&url)
            .send()
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .map_err(|e| e.to_string())?;

        if v["code"] != "Ok" {
            return Err(format!("OSRM respondió code={}", v["code"]));
        }
        let filas = v["distances"].as_array().ok_or("respuesta sin 'distances'")?;
        if filas.len() != puntos.len() {
            return Err("tamaño de matriz inesperado".into());
        }
        let mut m = Vec::with_capacity(filas.len());
        for fila in filas {
            let cols = fila.as_array().ok_or("fila inválida")?;
            if cols.len() != puntos.len() {
                return Err("tamaño de matriz inesperado".into());
            }
            let mut r = Vec::with_capacity(cols.len());
            for c in cols {
                // null = punto no alcanzable por la red vial
                r.push(c.as_f64().ok_or("distancia no disponible (punto fuera de la red vial)")? / 1000.0);
            }
            m.push(r);
        }
        Ok(m)
    }
}

impl ProveedorDistancias for Osrm {
    fn matriz_km(&self, puntos: &[Coordenada]) -> Result<(Matriz, &'static str), AppError> {
        match self.consultar(puntos) {
            Ok(m) => Ok((m, "osrm-osm")),
            Err(e) => {
                tracing::warn!("OSRM no disponible ({e}); se usa Haversine como respaldo");
                self.respaldo.matriz_km(puntos)
            }
        }
    }
}