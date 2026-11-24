//! Motor de optimización de rutas (P2, 10.1.2): vecino más cercano + mejora 2-opt.
//! Implementa el trait `OptimizadorRutas` definido en `control::puertos`.
//! Ruta abierta: parte del origen (índice 0) y termina en la última parada.
use std::{sync::Arc, time::Instant};

use super::distancias::{Matriz, ProveedorDistancias};
use crate::control::puertos::OptimizadorRutas;
use crate::entity::ruta::{RutaOptimizada, SolicitudRuta};
use crate::error::AppError;

const MAX_PASADAS: usize = 100;
const EPS: f64 = 1e-9;

pub struct Optimizador2Opt {
    distancias: Arc<dyn ProveedorDistancias>,
}

impl Optimizador2Opt {
    pub fn new(distancias: Arc<dyn ProveedorDistancias>) -> Self {
        Self { distancias }
    }
}

/// Longitud de un recorrido (lista de índices) según la matriz.
pub fn longitud(m: &Matriz, ruta: &[usize]) -> f64 {
    ruta.windows(2).map(|w| m[w[0]][w[1]]).sum()
}

/// Heurística constructiva: desde el origen (0), ir siempre al punto no visitado más cercano.
pub fn vecino_mas_cercano(m: &Matriz) -> Vec<usize> {
    let n = m.len();
    let mut visitado = vec![false; n];
    let mut ruta = Vec::with_capacity(n);
    if n == 0 {
        return ruta;
    }
    visitado[0] = true;
    ruta.push(0);
    let mut actual = 0;
    for _ in 1..n {
        let siguiente = (1..n)
            .filter(|&j| !visitado[j])
            .min_by(|&a, &b| m[actual][a].partial_cmp(&m[actual][b]).unwrap_or(std::cmp::Ordering::Equal));
        match siguiente {
            Some(j) => {
                visitado[j] = true;
                ruta.push(j);
                actual = j;
            }
            None => break,
        }
    }
    ruta
}

/// Mejora local 2-opt sobre ruta abierta con el origen fijo en la posición 0.
/// Requiere matriz simétrica. Nunca empeora la ruta de entrada.
pub fn dos_opt(m: &Matriz, ruta: &mut [usize]) {
    let n = ruta.len();
    if n < 3 {
        return;
    }
    let mut mejora = true;
    let mut pasadas = 0;
    while mejora && pasadas < MAX_PASADAS {
        mejora = false;
        pasadas += 1;
        for i in 1..n - 1 {
            for j in (i + 1)..n {
                let (a, b, c) = (ruta[i - 1], ruta[i], ruta[j]);
                // Invertir ruta[i..=j] cambia la arista (a,b) por (a,c) y, si existe, (c,d) por (b,d).
                let mut delta = m[a][c] - m[a][b];
                if j + 1 < n {
                    let d = ruta[j + 1];
                    delta += m[b][d] - m[c][d];
                }
                if delta < -EPS {
                    ruta[i..=j].reverse();
                    mejora = true;
                }
            }
        }
    }
}

/// Promedia m[i][j] y m[j][i]: las distancias viales pueden ser asimétricas (calles de un sentido).
fn simetrizar(m: &Matriz) -> Matriz {
    let n = m.len();
    let mut s = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            s[i][j] = (m[i][j] + m[j][i]) / 2.0;
        }
    }
    s
}

impl OptimizadorRutas for Optimizador2Opt {
    fn optimizar(&self, s: &SolicitudRuta) -> Result<RutaOptimizada, AppError> {
        let t = Instant::now();

        // Índice 0 = origen; índice k = parada k-1.
        let mut puntos = Vec::with_capacity(s.paradas.len() + 1);
        puntos.push(s.origen);
        puntos.extend(s.paradas.iter().map(|p| p.ubicacion));

        let (m, fuente) = self.distancias.matriz_km(&puntos)?;
        let sim = simetrizar(&m);

        let mut ruta = vecino_mas_cercano(&sim);
        dos_opt(&sim, &mut ruta);

        // La distancia reportada usa la matriz original (con asimetrías, si las hay).
        let total = longitud(&m, &ruta);
        let orden = ruta.iter().skip(1).map(|&i| s.paradas[i - 1].pedido_id.clone()).collect();

        Ok(RutaOptimizada {
            repartidor_id: s.repartidor_id.clone(),
            orden,
            distancia_km: (total * 1000.0).round() / 1000.0,
            algoritmo: format!("vecino-mas-cercano+2opt/{fuente}"),
            tiempo_calculo_ms: t.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::motor_rutas::distancias::{HaversineUrbano, ProveedorDistancias};
    use crate::entity::pedido::Coordenada;
    use crate::entity::ruta::Parada;

    fn parada(id: &str, lat: f64, lng: f64) -> Parada {
        Parada { pedido_id: id.into(), ubicacion: Coordenada { lat, lng } }
    }

    fn solicitud(paradas: Vec<Parada>) -> SolicitudRuta {
        SolicitudRuta {
            repartidor_id: "rep-1".into(),
            origen: Coordenada { lat: 13.70, lng: -89.20 },
            paradas,
        }
    }

    fn motor() -> Optimizador2Opt {
        Optimizador2Opt::new(Arc::new(HaversineUrbano::new(1.0)))
    }

    #[test]
    fn ordena_puntos_alineados() {
        let s = solicitud(vec![
            parada("p3", 13.70, -89.17),
            parada("p1", 13.70, -89.19),
            parada("p4", 13.70, -89.16),
            parada("p2", 13.70, -89.18),
        ]);
        let r = motor().optimizar(&s).unwrap();
        assert_eq!(r.orden, vec!["p1", "p2", "p3", "p4"]);
    }

    #[test]
    fn incluye_cada_parada_una_sola_vez() {
        let s = solicitud(vec![
            parada("a", 13.72, -89.15),
            parada("b", 13.68, -89.22),
            parada("c", 13.71, -89.19),
            parada("d", 13.67, -89.17),
            parada("e", 13.73, -89.21),
        ]);
        let r = motor().optimizar(&s).unwrap();
        let mut ids = r.orden.clone();
        ids.sort();
        assert_eq!(ids, vec!["a", "b", "c", "d", "e"]);
        assert!(r.distancia_km > 0.0);
    }

    #[test]
    fn una_parada_devuelve_esa_parada() {
        let r = motor().optimizar(&solicitud(vec![parada("unica", 13.71, -89.19)])).unwrap();
        assert_eq!(r.orden, vec!["unica"]);
    }

    #[test]
    fn dos_opt_nunca_empeora_vecino_mas_cercano() {
        let puntos = vec![
            Coordenada { lat: 13.70, lng: -89.20 },
            Coordenada { lat: 13.72, lng: -89.15 },
            Coordenada { lat: 13.68, lng: -89.22 },
            Coordenada { lat: 13.71, lng: -89.19 },
            Coordenada { lat: 13.67, lng: -89.17 },
            Coordenada { lat: 13.73, lng: -89.21 },
            Coordenada { lat: 13.69, lng: -89.14 },
        ];
        let (m, _) = HaversineUrbano::new(1.0).matriz_km(&puntos).unwrap();
        let mut ruta = vecino_mas_cercano(&m);
        let antes = longitud(&m, &ruta);
        dos_opt(&m, &mut ruta);
        assert!(longitud(&m, &ruta) <= antes + 1e-9);
        assert_eq!(ruta[0], 0);
    }
}