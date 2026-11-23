//! Configuración leída desde variables de entorno (nunca credenciales en el código).
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub couchdb_url: String,
    pub couchdb_user: String,
    pub couchdb_password: String,
    pub couchdb_db: String,
}

impl Config {
    pub fn desde_entorno() -> Self {
        let _ = dotenvy::dotenv();
        fn var(k: &str, defecto: &str) -> String {
            std::env::var(k).unwrap_or_else(|_| defecto.to_string())
        }
        Self {
            bind_addr: var("BIND_ADDR", "0.0.0.0:8080"),
            couchdb_url: var("COUCHDB_URL", "http://127.0.0.1:5984"),
            couchdb_user: var("COUCHDB_USER", "admin"),
            couchdb_password: var("COUCHDB_PASSWORD", ""),
            couchdb_db: var("COUCHDB_DB", "reparto"),
        }
    }
}
