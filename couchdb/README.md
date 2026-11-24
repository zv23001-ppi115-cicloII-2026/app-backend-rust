# CouchDB: persistencia y replicacion

## Documentos

`schemas/pedido.schema.json` describe el documento persistido por `Pedido`; `_rev` lo asigna CouchDB. El design document `design/reparto.json` aplica validaciones estructurales en el servidor. Las transiciones de estado se validan en `PedidoControl`, donde el backend puede consultar el estado vigente y aplicar la política de concurrencia.

`schemas/ruta-optimizada.schema.json` documenta la respuesta del optimizador. Las rutas no se persisten actualmente: `RutaControl` solo calcula y devuelve el resultado.

Al iniciar, Reqwest asegura la base, instala el design document y crea los indices Mango usados por `tipo`, `estado` y `repartidor_id`. El selector de pedidos usa siempre `tipo = pedido`.

## Replicacion de respaldo opcional

Por defecto la replicacion esta desactivada. Para conectar una segunda base CouchDB, configura `COUCHDB_REPLICATION_PEER_URL` con una URL de base completa, por ejemplo `https://couch.example:6984/reparto-movil`. Si el par requiere autenticacion, define tambien `COUCHDB_REPLICATION_PEER_USER` y `COUCHDB_REPLICATION_PEER_PASSWORD`; deben configurarse juntos. No pongas credenciales dentro de la URL.

El backend es la unica base de escritura para la API. El arranque mantiene una tarea continua y persistente en `_replicator` desde la base configurada en `COUCHDB_DB` hacia la base indicada por `COUCHDB_REPLICATION_PEER_URL`; elimina la tarea inversa reservada por la configuracion anterior. La base par es un espejo de respaldo y no debe recibir escrituras propias. Flutter consume la API Rust; esta replicacion no es un protocolo cliente-servidor para la app.

La replicacion refleja tambien actualizaciones y borrados, por lo que no conserva versiones historicas ante borrados accidentales. Para recuperacion ante perdida o corrupcion se necesitan ademas snapshots o copias independientes. Si ambas bases comparten instancia y almacenamiento, la replica no protege contra la perdida de ese servidor o volumen.

Usa credenciales de replicacion dedicadas y con privilegios minimos. CouchDB guarda la configuracion de autenticacion de la tarea en documentos de `_replicator`; por tanto, no uses credenciales administrativas de larga duracion como cuenta par.

## Conflictos

La API trata `hermeats` como autoridad y la base par como solo lectura. Al cambiar el estado, el backend usa control optimista con `_rev`: si una escritura concurrente gana, vuelve a leer el pedido y reevalua la transicion contra el estado mas reciente, hasta tres intentos. Si otro cliente ya aplico la misma asignacion al mismo repartidor, la solicitud es idempotente. Si la nueva transicion ya no es valida o el pedido se asigno a otro repartidor, devuelve `409` y conserva el cambio ganador. No hay politica de ultima escritura gana ni merge por timestamp.

Un `409` indica que la accion solicitada contradice el estado actual, que un ID de pedido se reutilizo con contenido distinto o que el documento sigue cambiando concurrentemente. Crear de nuevo un pedido con el mismo ID y los mismos datos de negocio es idempotente, incluso si su estado ya avanzo; esto permite reintentar solicitudes offline cuya respuesta se perdio. El lote aplica la misma regla. Si el mismo ID aparece con datos distintos, el backend lo rechaza sin sobrescribir el documento. Para otros `409`, el cliente debe volver a consultar el pedido y decidir si reintenta o informa al usuario; no debe reenviar ciegamente una copia completa desactualizada.

CouchDB conserva ramas concurrentes creadas por escrituras directas fuera de la API. La replicacion unidireccional y el respaldo sin escrituras propias evitan normalmente esa situacion. Si se modifica el respaldo manualmente o se escriben directamente documentos en CouchDB, las revisiones concurrentes no se fusionan automaticamente; mantenga la primaria como unico punto de escritura.

La identidad/autorizacion por dispositivo, filtrado de documentos por usuario y una API de cambios incremental siguen fuera del alcance de este backend actual. No expongas directamente la cuenta CouchDB administrativa a la app movil.