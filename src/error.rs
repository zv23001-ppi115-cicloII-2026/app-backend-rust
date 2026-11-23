//! Errores de la aplicación y su traducción a respuestas HTTP.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validacion(String),
    #[error("{0}")]
    NoEncontrado(String),
    #[error("{0}")]
    Conflicto(String),
    #[error("{0}")]
    Upstream(String),
    #[error("{0}")]
    Interno(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, codigo) = match &self {
            AppError::Validacion(_) => (StatusCode::BAD_REQUEST, "VALIDACION"),
            AppError::NoEncontrado(_) => (StatusCode::NOT_FOUND, "NO_ENCONTRADO"),
            AppError::Conflicto(_) => (StatusCode::CONFLICT, "CONFLICTO"),
            AppError::Upstream(_) => (StatusCode::BAD_GATEWAY, "COUCHDB_NO_DISPONIBLE"),
            AppError::Interno(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNO"),
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "error del servidor");
        }
        let cuerpo = json!({ "error": { "codigo": codigo, "mensaje": self.to_string() } });
        (status, Json(cuerpo)).into_response()
    }
}
