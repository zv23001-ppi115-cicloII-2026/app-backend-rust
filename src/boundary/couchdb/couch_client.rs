//! Cliente HTTP asíncrono (Reqwest) hacia CouchDB.
//! NOTA: este módulo se coordina con P3 (10.1.3). Si P3 entrega su propio cliente,
//! basta con que implemente `PedidoRepository`.
use std::time::Duration;

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{config::Config, error::AppError};

#[derive(Clone)]
pub struct CouchClient {
    http: Client,
    base: String,
    usuario: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct PutRespuesta {
    rev: String,
}

#[derive(Debug, Deserialize)]
pub struct BulkResultado {
    pub id: Option<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
struct FindRespuesta<T> {
    docs: Vec<T>,
}

impl CouchClient {
    pub fn new(cfg: &Config) -> Result<Self, AppError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(16)
            .build()
            .map_err(|e| AppError::Interno(format!("cliente HTTP: {e}")))?;
        Ok(Self {
            http,
            base: format!("{}/{}", cfg.couchdb_url.trim_end_matches('/'), cfg.couchdb_db),
            usuario: cfg.couchdb_user.clone(),
            password: cfg.couchdb_password.clone(),
        })
    }

    fn req(&self, metodo: Method, ruta: &str) -> RequestBuilder {
        let url = if ruta.is_empty() { self.base.clone() } else { format!("{}/{}", self.base, ruta) };
        self.http.request(metodo, url).basic_auth(&self.usuario, Some(&self.password))
    }

    async fn enviar(rb: RequestBuilder) -> Result<Response, AppError> {
        rb.send()
            .await
            .map_err(|e| AppError::Upstream(format!("CouchDB inalcanzable: {e}")))
    }

    async fn verificar(resp: Response) -> Result<Response, AppError> {
        let estado = resp.status();
        if estado.is_success() {
            return Ok(resp);
        }
        let cuerpo = resp.text().await.unwrap_or_default();
        Err(match estado {
            StatusCode::NOT_FOUND => AppError::NoEncontrado("Documento no encontrado".into()),
            StatusCode::CONFLICT => AppError::Conflicto("Conflicto de revisión (_rev desactualizada)".into()),
            _ => AppError::Upstream(format!("CouchDB respondió {estado}: {cuerpo}")),
        })
    }

    /// Crea la base de datos y el índice (tipo, estado) si no existen.
    pub async fn asegurar_db(&self) -> Result<(), AppError> {
        let r = Self::enviar(self.req(Method::PUT, "")).await?;
        if !(r.status().is_success() || r.status() == StatusCode::PRECONDITION_FAILED) {
            Self::verificar(r).await?;
        }
        let idx = json!({"index": {"fields": ["tipo", "estado"]}, "name": "idx-tipo-estado", "type": "json"});
        Self::verificar(Self::enviar(self.req(Method::POST, "_index").json(&idx)).await?).await?;
        Ok(())
    }

    /// Inserta o actualiza un documento (si incluye `_rev` actualiza). Devuelve la nueva `_rev`.
    pub async fn guardar<T: Serialize>(&self, id: &str, doc: &T) -> Result<String, AppError> {
        let resp = Self::verificar(Self::enviar(self.req(Method::PUT, id).json(doc)).await?).await?;
        let r: PutRespuesta = resp
            .json()
            .await
            .map_err(|e| AppError::Upstream(format!("respuesta inválida de CouchDB: {e}")))?;
        Ok(r.rev)
    }

    pub async fn leer<T: DeserializeOwned>(&self, id: &str) -> Result<T, AppError> {
        let resp = Self::verificar(Self::enviar(self.req(Method::GET, id)).await?).await?;
        resp.json()
            .await
            .map_err(|e| AppError::Upstream(format!("documento ilegible: {e}")))
    }

    pub async fn buscar<T: DeserializeOwned>(&self, selector: Value, limite: u32) -> Result<Vec<T>, AppError> {
        let cuerpo = json!({"selector": selector, "limit": limite});
        let resp = Self::verificar(Self::enviar(self.req(Method::POST, "_find").json(&cuerpo)).await?).await?;
        let r: FindRespuesta<T> = resp
            .json()
            .await
            .map_err(|e| AppError::Upstream(format!("respuesta _find inválida: {e}")))?;
        Ok(r.docs)
    }

    pub async fn guardar_lote<T: Serialize>(&self, docs: &[T]) -> Result<Vec<BulkResultado>, AppError> {
        let cuerpo = json!({ "docs": docs });
        let resp = Self::verificar(Self::enviar(self.req(Method::POST, "_bulk_docs").json(&cuerpo)).await?).await?;
        resp.json()
            .await
            .map_err(|e| AppError::Upstream(format!("respuesta _bulk_docs inválida: {e}")))
    }
}
