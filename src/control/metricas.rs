//! Instrumentación: latencia por endpoint (media, p50, p95, p99, máx), throughput de documentos
//! (ventana de 60 s) y RAM/CPU del proceso. Alimenta los apartados 10.2 y 10.3.
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sysinfo::{Pid, System};

const MAX_MUESTRAS: usize = 10_000;
const VENTANA: Duration = Duration::from_secs(60);

#[derive(Default)]
struct EstadisticaRuta {
    total: u64,
    errores_5xx: u64,
    suma_ms: f64,
    max_ms: f64,
    muestras: VecDeque<f64>,
}

#[derive(Default)]
struct Interno {
    rutas: HashMap<String, EstadisticaRuta>,
    docs_eventos: VecDeque<(Instant, u64)>,
    docs_total: u64,
}

pub struct Metricas {
    inicio: Instant,
    interno: Mutex<Interno>,
    sys: Mutex<System>,
    proceso: Mutex<(f64, f32)>, // (ram_mb, cpu_pct)
    pid: Pid,
}

#[derive(Serialize)]
pub struct SnapshotEndpoint {
    pub endpoint: String,
    pub total: u64,
    pub errores_5xx: u64,
    pub media_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Serialize)]
pub struct SnapshotProceso {
    pub ram_mb: f64,
    pub cpu_pct: f32,
}

#[derive(Serialize)]
pub struct SnapshotDocs {
    pub total: u64,
    pub por_segundo_60s: f64,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub generado_en: DateTime<Utc>,
    pub uptime_s: u64,
    pub proceso: SnapshotProceso,
    pub documentos: SnapshotDocs,
    pub endpoints: Vec<SnapshotEndpoint>,
}

fn percentil(ordenado: &[f64], p: f64) -> f64 {
    if ordenado.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (ordenado.len() as f64 - 1.0)).round() as usize;
    ordenado[idx.min(ordenado.len() - 1)]
}

impl Metricas {
    pub fn new() -> Self {
        Self {
            inicio: Instant::now(),
            interno: Mutex::new(Interno::default()),
            sys: Mutex::new(System::new()),
            proceso: Mutex::new((0.0, 0.0)),
            pid: Pid::from_u32(std::process::id()),
        }
    }

    /// Llamado por el middleware en cada petición.
    pub fn registrar(&self, endpoint: &str, ms: f64, status: u16) {
        let mut i = self.interno.lock().unwrap();
        let e = i.rutas.entry(endpoint.to_string()).or_default();
        e.total += 1;
        e.suma_ms += ms;
        if ms > e.max_ms {
            e.max_ms = ms;
        }
        if status >= 500 {
            e.errores_5xx += 1;
        }
        if e.muestras.len() >= MAX_MUESTRAS {
            e.muestras.pop_front();
        }
        e.muestras.push_back(ms);
    }

    /// Llamado por los handlers que persisten documentos (throughput docs/s).
    pub fn contar_docs(&self, n: u64) {
        let mut i = self.interno.lock().unwrap();
        let ahora = Instant::now();
        i.docs_total += n;
        i.docs_eventos.push_back((ahora, n));
        while let Some((t, _)) = i.docs_eventos.front() {
            if ahora.duration_since(*t) > VENTANA {
                i.docs_eventos.pop_front();
            } else {
                break;
            }
        }
    }

    /// Se invoca cada segundo desde una tarea de fondo (ver main.rs).
    pub fn refrescar_proceso(&self) {
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_process(self.pid);
        if let Some(p) = sys.process(self.pid) {
            *self.proceso.lock().unwrap() = (p.memory() as f64 / 1_048_576.0, p.cpu_usage());
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let uptime = self.inicio.elapsed();
        let (ram_mb, cpu_pct) = *self.proceso.lock().unwrap();
        let i = self.interno.lock().unwrap();
        let ahora = Instant::now();
        let docs_ventana: u64 = i
            .docs_eventos
            .iter()
            .filter(|(t, _)| ahora.duration_since(*t) <= VENTANA)
            .map(|(_, n)| *n)
            .sum();
        let ventana_s = uptime.as_secs_f64().clamp(1.0, VENTANA.as_secs_f64());
        let mut endpoints: Vec<SnapshotEndpoint> = i
            .rutas
            .iter()
            .map(|(k, e)| {
                let mut v: Vec<f64> = e.muestras.iter().copied().collect();
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                SnapshotEndpoint {
                    endpoint: k.clone(),
                    total: e.total,
                    errores_5xx: e.errores_5xx,
                    media_ms: if e.total > 0 { e.suma_ms / e.total as f64 } else { 0.0 },
                    p50_ms: percentil(&v, 50.0),
                    p95_ms: percentil(&v, 95.0),
                    p99_ms: percentil(&v, 99.0),
                    max_ms: e.max_ms,
                }
            })
            .collect();
        endpoints.sort_by(|a, b| a.endpoint.cmp(&b.endpoint));
        Snapshot {
            generado_en: Utc::now(),
            uptime_s: uptime.as_secs(),
            proceso: SnapshotProceso { ram_mb, cpu_pct },
            documentos: SnapshotDocs { total: i.docs_total, por_segundo_60s: docs_ventana as f64 / ventana_s },
            endpoints,
        }
    }
}
