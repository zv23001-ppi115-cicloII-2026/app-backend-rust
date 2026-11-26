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

#[derive(Debug, Serialize)]
pub struct ArqueoRepartidor {
    pub repartidor_id: String,
    pub entregas_completadas: usize,
    pub entregas_en_curso: usize,
    pub efectivo_a_rendir: f64,
    pub otros_metodos: f64,
    pub total_gestionado: f64,
}

#[derive(Debug, Serialize)]
pub struct ResumenOperativo {
    pub total_pedidos: usize,
    pub entregados: usize,
    pub en_ruta: usize,
    pub pendientes: usize,
    pub cancelados: usize,
    pub ventas_brutas: f64,
    pub subtotal_productos: f64,
    pub ingresos_envio: f64,
    pub total_efectivo: f64,
    pub total_transferencia_pos: f64,
    pub ticket_promedio: f64,
    pub arqueo_por_repartidor: Vec<ArqueoRepartidor>,
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

    pub async fn resumen_operativo(&self) -> Result<ResumenOperativo, AppError> {
        let pedidos = self
            .repo
            .listar(FiltroPedidos {
                estado: None,
                repartidor_id: None,
                limite: MAX_LISTADO,
            })
            .await?;

        let mut entregados = 0;
        let mut en_ruta = 0;
        let mut pendientes = 0;
        let mut cancelados = 0;

        let mut ventas_brutas = 0.0;
        let mut subtotal_productos = 0.0;
        let mut ingresos_envio = 0.0;
        let mut total_efectivo = 0.0;
        let mut total_transferencia_pos = 0.0;

        let mut mapa_rep: HashMap<String, ArqueoRepartidor> = HashMap::new();

        for p in &pedidos {
            match p.estado {
                EstadoPedido::Entregado => entregados += 1,
                EstadoPedido::EnRuta | EstadoPedido::Asignado => en_ruta += 1,
                EstadoPedido::Pendiente => pendientes += 1,
                EstadoPedido::Cancelado => cancelados += 1,
            }

            if p.estado != EstadoPedido::Cancelado {
                let envio = p.costo_envio.unwrap_or(0.0);
                let sub = p.subtotal_productos.unwrap_or((p.total - envio).max(0.0));
                ventas_brutas += p.total;
                subtotal_productos += sub;
                ingresos_envio += envio;

                if p.metodo_pago.eq_ignore_ascii_case("efectivo") {
                    total_efectivo += p.total;
                } else {
                    total_transferencia_pos += p.total;
                }
            }

            if let Some(ref rep_id) = p.repartidor_id {
                let entry = mapa_rep
                    .entry(rep_id.clone())
                    .or_insert_with(|| ArqueoRepartidor {
                        repartidor_id: rep_id.clone(),
                        entregas_completadas: 0,
                        entregas_en_curso: 0,
                        efectivo_a_rendir: 0.0,
                        otros_metodos: 0.0,
                        total_gestionado: 0.0,
                    });

                match p.estado {
                    EstadoPedido::Entregado => {
                        entry.entregas_completadas += 1;
                        entry.total_gestionado += p.total;
                        if p.metodo_pago.eq_ignore_ascii_case("efectivo") {
                            entry.efectivo_a_rendir += p.total;
                        } else {
                            entry.otros_metodos += p.total;
                        }
                    }
                    EstadoPedido::Asignado | EstadoPedido::EnRuta => {
                        entry.entregas_en_curso += 1;
                    }
                    _ => {}
                }
            }
        }

        let activos = pedidos.len().saturating_sub(cancelados);
        let ticket_promedio = if activos > 0 {
            ventas_brutas / (activos as f64)
        } else {
            0.0
        };

        let mut arqueo_por_repartidor: Vec<ArqueoRepartidor> = mapa_rep.into_values().collect();
        arqueo_por_repartidor.sort_by(|a, b| a.repartidor_id.cmp(&b.repartidor_id));

        Ok(ResumenOperativo {
            total_pedidos: pedidos.len(),
            entregados,
            en_ruta,
            pendientes,
            cancelados,
            ventas_brutas,
            subtotal_productos,
            ingresos_envio,
            total_efectivo,
            total_transferencia_pos,
            ticket_promedio,
            arqueo_por_repartidor,
        })
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
            if nuevo == EstadoPedido::EnRuta && pedido.inicio_ruta_en.is_none() {
                pedido.inicio_ruta_en = Some(ahora);
            }
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