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
    server_base: String,
    base: String,
    db: String,
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
            server_base: cfg.couchdb_url.trim_end_matches('/').to_string(),
            base: format!(
                "{}/{}",
                cfg.couchdb_url.trim_end_matches('/'),
                cfg.couchdb_db
            ),
            db: cfg.couchdb_db.clone(),
            usuario: cfg.couchdb_user.clone(),
            password: cfg.couchdb_password.clone(),
        })
    }

    fn req(&self, metodo: Method, ruta: &str) -> RequestBuilder {
        let url = if ruta.is_empty() {
            self.base.clone()
        } else {
            format!("{}/{}", self.base, ruta)
        };
        self.http
            .request(metodo, url)
            .basic_auth(&self.usuario, Some(&self.password))
    }

    fn req_servidor(&self, metodo: Method, ruta: &str) -> RequestBuilder {
        self.http
            .request(metodo, format!("{}/{}", self.server_base, ruta))
            .basic_auth(&self.usuario, Some(&self.password))
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
            StatusCode::CONFLICT => {
                AppError::Conflicto("Conflicto de revisión (_rev desactualizada)".into())
            }
            _ => AppError::Upstream(format!("CouchDB respondió {estado}: {cuerpo}")),
        })
    }

    /// Asegura base, validación de documentos e índices Mango requeridos por los filtros.
    pub async fn asegurar_db(&self) -> Result<(), AppError> {
        let r = Self::enviar(self.req(Method::PUT, "")).await?;
        if !(r.status().is_success() || r.status() == StatusCode::PRECONDITION_FAILED) {
            Self::verificar(r).await?;
        }
        let design_doc: Value =
            serde_json::from_str(include_str!("../../../couchdb/design/reparto.json"))
                .map_err(|e| AppError::Interno(format!("design document inválido: {e}")))?;
        self.asegurar_documento("_design/reparto", design_doc, false)
            .await?;

        let indices = [
            include_str!("../../../couchdb/indexes/pedidos-por-estado.json"),
            include_str!("../../../couchdb/indexes/pedidos-por-repartidor.json"),
            include_str!("../../../couchdb/indexes/pedidos-por-estado-y-repartidor.json"),
        ];
        for definicion in indices {
            let indice: Value = serde_json::from_str(definicion)
                .map_err(|e| AppError::Interno(format!("definición de índice inválida: {e}")))?;
            Self::verificar(Self::enviar(self.req(Method::POST, "_index").json(&indice)).await?)
                .await?;
        }
        Ok(())
    }

    /// Configura replicación continua unidireccional de la base primaria a la base de respaldo.
    pub async fn configurar_replicacion(
        &self,
        peer_url: &str,
        peer_user: Option<&str>,
        peer_password: Option<&str>,
    ) -> Result<(), AppError> {
        let mut peer = reqwest::Url::parse(peer_url).map_err(|e| {
            AppError::Validacion(format!("COUCHDB_REPLICATION_PEER_URL inválida: {e}"))
        })?;
        if !matches!(peer.scheme(), "http" | "https")
            || peer.host_str().is_none()
            || peer.path().trim_matches('/').is_empty()
            || !peer.username().is_empty()
            || peer.password().is_some()
            || peer.query().is_some()
            || peer.fragment().is_some()
        {
            return Err(AppError::Validacion(
                "la URL par debe ser http(s)://host:puerto/base y no incluir credenciales, query ni fragmento".into(),
            ));
        }
        let peer_path = peer.path().trim_end_matches('/').to_string();
        peer.set_path(&peer_path);
        let peer_url = peer.to_string();
        let credenciales = match (peer_user, peer_password) {
            (Some(usuario), Some(password)) => Some(json!({
                "auth": { "basic": { "username": usuario, "password": password } }
            })),
            (None, None) => None,
            _ => return Err(AppError::Validacion(
                "COUCHDB_REPLICATION_PEER_USER y COUCHDB_REPLICATION_PEER_PASSWORD deben definirse juntos".into(),
            )),
        };

        let endpoint_local = json!({
            "url": format!("{}/{}", self.server_base, self.db),
            "auth": { "basic": { "username": self.usuario, "password": self.password } }
        });
        let mut endpoint_par = json!({ "url": peer_url });
        if let Some(auth) = credenciales {
            endpoint_par["auth"] = auth["auth"].clone();
        }
        let replica = json!({
            "_id": "reparto-a-par",
            "source": endpoint_local,
            "target": endpoint_par,
            "continuous": true,
            "create_target": true
        });
        self.asegurar_documento("_replicator/reparto-a-par", replica, true)
            .await?;
        self.eliminar_documento_servidor("_replicator/par-a-reparto")
            .await?;
        Ok(())
    }

    async fn eliminar_documento_servidor(&self, ruta: &str) -> Result<(), AppError> {
        let respuesta = Self::enviar(self.req_servidor(Method::GET, ruta)).await?;
        if respuesta.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        let respuesta = Self::verificar(respuesta).await?;
        let documento: Value = respuesta
            .json()
            .await
            .map_err(|e| AppError::Upstream(format!("documento CouchDB ilegible: {e}")))?;
        let rev = documento
            .get("_rev")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Upstream("documento CouchDB existente sin _rev".into()))?;
        let respuesta =
            Self::enviar(self.req_servidor(Method::DELETE, &format!("{ruta}?rev={rev}"))).await?;
        Self::verificar(respuesta).await?;
        Ok(())
    }

    async fn asegurar_documento(
        &self,
        ruta: &str,
        mut documento: Value,
        servidor: bool,
    ) -> Result<(), AppError> {
        let obtener = if servidor {
            self.req_servidor(Method::GET, ruta)
        } else {
            self.req(Method::GET, ruta)
        };
        let respuesta = Self::enviar(obtener).await?;
        if respuesta.status() != StatusCode::NOT_FOUND {
            let respuesta = Self::verificar(respuesta).await?;
            let existente: Value = respuesta
                .json()
                .await
                .map_err(|e| AppError::Upstream(format!("documento CouchDB ilegible: {e}")))?;
            documento["_rev"] = existente
                .get("_rev")
                .cloned()
                .ok_or_else(|| AppError::Upstream("documento CouchDB existente sin _rev".into()))?;
        }
        let guardar = if servidor {
            self.req_servidor(Method::PUT, ruta)
        } else {
            self.req(Method::PUT, ruta)
        };
        Self::verificar(Self::enviar(guardar.json(&documento)).await?).await?;
        Ok(())
    }

    /// Inserta o actualiza un documento (si incluye `_rev` actualiza). Devuelve la nueva `_rev`.
    pub async fn guardar<T: Serialize>(&self, id: &str, doc: &T) -> Result<String, AppError> {
        let resp =
            Self::verificar(Self::enviar(self.req(Method::PUT, id).json(doc)).await?).await?;
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

    pub async fn buscar<T: DeserializeOwned>(
        &self,
        selector: Value,
        limite: u32,
    ) -> Result<Vec<T>, AppError> {
        let cuerpo = json!({"selector": selector, "limit": limite});
        let resp =
            Self::verificar(Self::enviar(self.req(Method::POST, "_find").json(&cuerpo)).await?)
                .await?;
        let r: FindRespuesta<T> = resp
            .json()
            .await
            .map_err(|e| AppError::Upstream(format!("respuesta _find inválida: {e}")))?;
        Ok(r.docs)
    }

    pub async fn guardar_lote<T: Serialize>(
        &self,
        docs: &[T],
    ) -> Result<Vec<BulkResultado>, AppError> {
        let cuerpo = json!({ "docs": docs });
        let resp = Self::verificar(
            Self::enviar(self.req(Method::POST, "_bulk_docs").json(&cuerpo)).await?,
        )
        .await?;
        resp.json()
            .await
            .map_err(|e| AppError::Upstream(format!("respuesta _bulk_docs inválida: {e}")))
    }
}
