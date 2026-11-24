//! Configuración leída desde variables de entorno (nunca credenciales en el código).
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub couchdb_url: String,
    pub couchdb_user: String,
    pub couchdb_password: String,
    pub couchdb_db: String,
    pub couchdb_replication_peer_url: Option<String>,
    pub couchdb_replication_peer_user: Option<String>,
    pub couchdb_replication_peer_password: Option<String>,
    pub osrm_url: Option<String>,
    pub osrm_timeout_ms: u64,
    pub factor_circuito: f64,
}

impl Config {
    pub fn desde_entorno() -> Self {
        let _ = dotenvy::dotenv();
        fn var(k: &str, defecto: &str) -> String {
            std::env::var(k).unwrap_or_else(|_| defecto.to_string())
        }
        fn opcional(k: &str) -> Option<String> {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        }
        Self {
            bind_addr: var("BIND_ADDR", "0.0.0.0:8080"),
            couchdb_url: var("COUCHDB_URL", "http://127.0.0.1:5984"),
            couchdb_user: var("COUCHDB_USER", "admin"),
            couchdb_password: var("COUCHDB_PASSWORD", ""),
            couchdb_db: var("COUCHDB_DB", "reparto"),
            couchdb_replication_peer_url: opcional("COUCHDB_REPLICATION_PEER_URL"),
            couchdb_replication_peer_user: opcional("COUCHDB_REPLICATION_PEER_USER"),
            couchdb_replication_peer_password: opcional("COUCHDB_REPLICATION_PEER_PASSWORD"),
            osrm_url: std::env::var("OSRM_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            osrm_timeout_ms: var("OSRM_TIMEOUT_MS", "2000").parse().unwrap_or(2000),
            factor_circuito: var("FACTOR_CIRCUITO", "1.3").parse().unwrap_or(1.3),
        }
    }
}
