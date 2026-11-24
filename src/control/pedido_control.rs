//! Control: casos de uso de pedidos. Orquesta las entidades y el puerto de persistencia.
use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::puertos::{FiltroPedidos, PedidoRepository, ResultadoDoc};
use crate::entity::pedido::{id_valido, EstadoPedido, NuevoPedido, Pedido};
use crate::error::AppError;

const MAX_LOTE: usize = 500;
const MAX_LISTADO: u32 = 500;
const MAX_REINTENTOS_CONFLICTO: usize = 3;

#[derive(Debug, Serialize)]
pub struct ResultadoLote {
    pub aceptados: usize,
    pub rechazados: Vec<ResultadoDoc>,
}

pub struct PedidoControl {
    repo: Arc<dyn PedidoRepository>,
}

impl PedidoControl {
    pub fn new(repo: Arc<dyn PedidoRepository>) -> Self {
        Self { repo }
    }

    pub async fn crear(&self, entrada: NuevoPedido) -> Result<Pedido, AppError> {
        let pedido = Pedido::nuevo(entrada)?;
        match self.repo.crear(pedido.clone()).await {
            Ok(creado) => Ok(creado),
            Err(AppError::Conflicto(_)) => {
                let existente = self.repo.obtener(&pedido.id).await?;
                if mismo_pedido_para_reintento(&existente, &pedido) {
                    Ok(existente)
                } else {
                    Err(AppError::Conflicto(
                        "el id del pedido ya existe con datos distintos; no se sobrescribió".into(),
                    ))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub async fn obtener(&self, id: &str) -> Result<Pedido, AppError> {
        if !id_valido(id) {
            return Err(AppError::Validacion("id con formato inválido".into()));
        }
        self.repo.obtener(id).await
    }

    pub async fn listar(&self, mut filtro: FiltroPedidos) -> Result<Vec<Pedido>, AppError> {
        if filtro.limite == 0 || filtro.limite > MAX_LISTADO {
            filtro.limite = 100;
        }
        self.repo.listar(filtro).await
    }

    pub async fn cambiar_estado(
        &self,
        id: &str,
        nuevo: EstadoPedido,
        repartidor_id: Option<String>,
        entregado_en: Option<DateTime<Utc>>,
    ) -> Result<Pedido, AppError> {
        for intento in 0..MAX_REINTENTOS_CONFLICTO {
            let mut pedido = self.obtener(id).await?;
            if pedido.estado == nuevo {
                if nuevo == EstadoPedido::Asignado {
                    if let Some(solicitado) = repartidor_id.as_ref() {
                        if !id_valido(solicitado) {
                            return Err(AppError::Validacion("repartidor_id inválido".into()));
                        }
                        if pedido.repartidor_id.as_ref() != Some(solicitado) {
                            return Err(AppError::Conflicto(
                                "el pedido ya fue asignado a otro repartidor".into(),
                            ));
                        }
                    }
                }
                return Ok(pedido);
            }
            if !pedido.estado.puede_pasar_a(nuevo) {
                return Err(AppError::Conflicto(format!(
                    "Transición no permitida con el estado actual: {} -> {}",
                    pedido.estado.como_str(),
                    nuevo.como_str()
                )));
            }
            if nuevo == EstadoPedido::Asignado {
                let r = repartidor_id
                    .as_ref()
                    .filter(|r| id_valido(r))
                    .cloned()
                    .ok_or_else(|| {
                        AppError::Validacion("repartidor_id es obligatorio al asignar".into())
                    })?;
                pedido.repartidor_id = Some(r);
            }
            let ahora = Utc::now();
            if nuevo == EstadoPedido::Entregado {
                pedido.entregado_en = Some(entregado_en.clone().unwrap_or(ahora));
            }
            pedido.estado = nuevo;
            pedido.actualizado_en = ahora;
            match self.repo.actualizar(pedido).await {
                Ok(actualizado) => return Ok(actualizado),
                Err(AppError::Conflicto(_)) if intento + 1 < MAX_REINTENTOS_CONFLICTO => continue,
                Err(AppError::Conflicto(_)) => {
                    return Err(AppError::Conflicto(
                        "el pedido sigue cambiando concurrentemente; vuelve a consultar e intenta de nuevo".into(),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("el bucle siempre devuelve o reintenta")
    }

    /// Ingesta masiva (p. ej. cola acumulada offline). Valida cada documento y persiste en una sola operación.
    pub async fn crear_lote(&self, entradas: Vec<NuevoPedido>) -> Result<ResultadoLote, AppError> {
        if entradas.is_empty() || entradas.len() > MAX_LOTE {
            return Err(AppError::Validacion(format!(
                "El lote debe tener entre 1 y {MAX_LOTE} pedidos"
            )));
        }
        let mut validos = Vec::with_capacity(entradas.len());
        let mut rechazados = Vec::new();
        let mut por_id = HashMap::new();
        for (i, e) in entradas.into_iter().enumerate() {
            let id_ref = e.id.clone().unwrap_or_else(|| format!("#{i}"));
            match Pedido::nuevo(e) {
                Ok(p) => {
                    if let Some(anterior) = por_id.get(&p.id) {
                        if !mismo_pedido_para_reintento(anterior, &p) {
                            rechazados.push(ResultadoDoc {
                                id: p.id,
                                ok: false,
                                razon: Some("el lote repite un id con datos distintos".into()),
                            });
                        }
                    } else {
                        por_id.insert(p.id.clone(), p.clone());
                        validos.push(p);
                    }
                }
                Err(err) => rechazados.push(ResultadoDoc {
                    id: id_ref,
                    ok: false,
                    razon: Some(err.to_string()),
                }),
            }
        }
        let mut aceptados = 0;
        if !validos.is_empty() {
            for r in self.repo.crear_lote(validos).await? {
                if r.ok {
                    aceptados += 1;
                } else {
                    match por_id.get(&r.id) {
                        Some(intento) => match self.repo.obtener(&r.id).await {
                            Ok(existente) if mismo_pedido_para_reintento(&existente, intento) => {
                                aceptados += 1;
                            }
                            Ok(_) => rechazados.push(ResultadoDoc {
                                id: r.id,
                                ok: false,
                                razon: Some(
                                    "el id del pedido ya existe con datos distintos; no se sobrescribió".into(),
                                ),
                            }),
                            Err(AppError::NoEncontrado(_)) => rechazados.push(r),
                            Err(error) => return Err(error),
                        },
                        None => rechazados.push(r),
                    }
                }
            }
        }
        Ok(ResultadoLote {
            aceptados,
            rechazados,
        })
    }
}

fn mismo_pedido_para_reintento(actual: &Pedido, solicitado: &Pedido) -> bool {
    actual.id == solicitado.id
        && actual.tipo == solicitado.tipo
        && actual.cliente == solicitado.cliente
        && actual.direccion == solicitado.direccion
        && actual.ubicacion.lat == solicitado.ubicacion.lat
        && actual.ubicacion.lng == solicitado.ubicacion.lng
        && actual.total == solicitado.total
}

#[cfg(test)]
mod pruebas_conflicto {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::{
        control::puertos::{FiltroPedidos, ResultadoDoc},
        entity::pedido::{Coordenada, NuevoPedido},
    };

    struct RepoConActualizacionConcurrente {
        pedido: Mutex<Pedido>,
        version_concurrente: Mutex<Option<Pedido>>,
    }

    #[async_trait]
    impl PedidoRepository for RepoConActualizacionConcurrente {
        async fn crear(&self, _pedido: Pedido) -> Result<Pedido, AppError> {
            Err(AppError::Conflicto("id existente".into()))
        }

        async fn obtener(&self, _id: &str) -> Result<Pedido, AppError> {
            Ok(self.pedido.lock().unwrap().clone())
        }

        async fn listar(&self, _filtro: FiltroPedidos) -> Result<Vec<Pedido>, AppError> {
            Ok(vec![self.pedido.lock().unwrap().clone()])
        }

        async fn actualizar(&self, pedido: Pedido) -> Result<Pedido, AppError> {
            if let Some(version) = self.version_concurrente.lock().unwrap().take() {
                *self.pedido.lock().unwrap() = version;
                return Err(AppError::Conflicto("_rev desactualizada".into()));
            }
            *self.pedido.lock().unwrap() = pedido.clone();
            Ok(pedido)
        }

        async fn crear_lote(&self, pedidos: Vec<Pedido>) -> Result<Vec<ResultadoDoc>, AppError> {
            Ok(pedidos
                .into_iter()
                .map(|pedido| ResultadoDoc {
                    id: pedido.id,
                    ok: false,
                    razon: Some("ya existe un documento con ese id".into()),
                })
                .collect())
        }
    }

    fn entrada(id: &str, total: f64) -> NuevoPedido {
        NuevoPedido {
            id: Some(id.into()),
            cliente: "Cliente".into(),
            direccion: "Direccion".into(),
            ubicacion: Coordenada {
                lat: 13.7,
                lng: -89.2,
            },
            total,
            creado_en: None,
        }
    }

    fn pedido_pendiente() -> Pedido {
        Pedido::nuevo(entrada("pedido:conflicto", 1.0)).unwrap()
    }

    fn repo_con_conflicto(version_concurrente: Pedido) -> Arc<RepoConActualizacionConcurrente> {
        Arc::new(RepoConActualizacionConcurrente {
            pedido: Mutex::new(pedido_pendiente()),
            version_concurrente: Mutex::new(Some(version_concurrente)),
        })
    }

    #[tokio::test]
    async fn reintento_idempotente_acepta_la_misma_asignacion_concurrente() {
        let mut version_concurrente = pedido_pendiente();
        version_concurrente.estado = EstadoPedido::Asignado;
        version_concurrente.repartidor_id = Some("rep-1".into());
        version_concurrente.rev = Some("2-concurrente".into());
        let repo = repo_con_conflicto(version_concurrente);
        let control = PedidoControl::new(repo.clone());

        let resultado = control
            .cambiar_estado(
                "pedido:conflicto",
                EstadoPedido::Asignado,
                Some("rep-1".into()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(resultado.estado, EstadoPedido::Asignado);
        assert_eq!(resultado.repartidor_id.as_deref(), Some("rep-1"));
        assert_eq!(resultado.rev.as_deref(), Some("2-concurrente"));
    }

    #[tokio::test]
    async fn reintento_rechaza_asignacion_concurrente_a_otro_repartidor() {
        let mut version_concurrente = pedido_pendiente();
        version_concurrente.estado = EstadoPedido::Asignado;
        version_concurrente.repartidor_id = Some("rep-2".into());
        version_concurrente.rev = Some("2-concurrente".into());
        let repo = repo_con_conflicto(version_concurrente);
        let control = PedidoControl::new(repo.clone());

        let resultado = control
            .cambiar_estado(
                "pedido:conflicto",
                EstadoPedido::Asignado,
                Some("rep-1".into()),
                None,
            )
            .await;

        assert!(
            matches!(resultado, Err(AppError::Conflicto(mensaje)) if mensaje.contains("otro repartidor"))
        );
        assert_eq!(
            repo.pedido.lock().unwrap().repartidor_id.as_deref(),
            Some("rep-2")
        );
    }

    #[tokio::test]
    async fn creacion_repetida_con_el_mismo_id_y_contenido_es_idempotente() {
        let mut existente = pedido_pendiente();
        existente.estado = EstadoPedido::Asignado;
        existente.repartidor_id = Some("rep-1".into());
        existente.rev = Some("2-existente".into());
        let repo = Arc::new(RepoConActualizacionConcurrente {
            pedido: Mutex::new(existente),
            version_concurrente: Mutex::new(None),
        });
        let control = PedidoControl::new(repo);

        let resultado = control
            .crear(entrada("pedido:conflicto", 1.0))
            .await
            .unwrap();

        assert_eq!(resultado.estado, EstadoPedido::Asignado);
        assert_eq!(resultado.rev.as_deref(), Some("2-existente"));
    }

    #[tokio::test]
    async fn creacion_repetida_con_datos_distintos_no_sobrescribe() {
        let repo = repo_con_conflicto(pedido_pendiente());
        let control = PedidoControl::new(repo.clone());

        let resultado = control.crear(entrada("pedido:conflicto", 9.0)).await;

        assert!(
            matches!(resultado, Err(AppError::Conflicto(mensaje)) if mensaje.contains("datos distintos"))
        );
        assert_eq!(repo.pedido.lock().unwrap().total, 1.0);
    }

    #[tokio::test]
    async fn lote_reintentado_acepta_el_pedido_existente_con_el_mismo_contenido() {
        let repo = repo_con_conflicto(pedido_pendiente());
        let control = PedidoControl::new(repo);

        let resultado = control
            .crear_lote(vec![entrada("pedido:conflicto", 1.0)])
            .await
            .unwrap();

        assert_eq!(resultado.aceptados, 1);
        assert!(resultado.rechazados.is_empty());
    }
}
