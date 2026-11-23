//! Middleware de medición: latencia por petición, cabecera `X-Response-Time-Ms` y traza estructurada
//! con target "metrica" (una línea por petición, apta para recolección en 10.3).
use std::{sync::Arc, time::Instant};

use axum::{
    extract::{MatchedPath, Request, State},
    http::HeaderValue,
    middleware::Next,
    response::Response,
};

use crate::control::metricas::Metricas;

pub async fn medir(State(m): State<Arc<Metricas>>, req: Request, next: Next) -> Response {
    let metodo = req.method().clone();
    let ruta = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());
    let clave = format!("{metodo} {ruta}");
    let t = Instant::now();

    let mut resp = next.run(req).await;

    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let status = resp.status().as_u16();
    m.registrar(&clave, ms, status);
    tracing::info!(target: "metrica", endpoint = %clave, status, ms = format!("{ms:.3}"));
    if let Ok(v) = HeaderValue::from_str(&format!("{ms:.3}")) {
        resp.headers_mut().insert("x-response-time-ms", v);
    }
    resp
}
