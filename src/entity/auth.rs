use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub usuario: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocumentoUsuarios {
    #[serde(default)]
    pub usuarios: Vec<UsuarioRegistro>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsuarioRegistro {
    pub usuario: String,
    pub password: String,
    pub rol: String,
    pub nombre: String,
    pub repartidor_id: Option<String>,
    pub negocio_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SesionUsuario {
    pub usuario: String,
    pub rol: String,
    pub nombre: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repartidor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negocio_id: Option<String>,
}