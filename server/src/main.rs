mod client;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use slog_async::OverflowStrategy;
use std::{
    cell::UnsafeCell, collections::HashMap, sync::atomic::AtomicBool, sync::atomic::AtomicU64,
    sync::atomic::Ordering, sync::Arc, sync::RwLock,
};
use warp::{ws::Message, Filter};

use cao_queue::{
    collections::spmcfifo::SpmcFifo, commands::Command, message::OwnedMessage, MessageId,
};
use slog::{debug, info, warn, Drain, Logger};

/// Collection of queues by name
type Exchange = Arc<RwLock<HashMap<QueueName, Arc<SpmcQueue>>>>;
type QueueName = String; // TODO: short string?

/// Single producer - multi consumer queue
pub struct SpmcQueue {
    pub next_id: UnsafeCell<MessageId>,
    pub queue: SpmcFifo<OwnedMessage>,
    pub has_producer: AtomicBool,
    /// number of connected, active clients
    pub clients: AtomicU64,
    pub name: QueueName,
}

unsafe impl<'a> Sync for SpmcQueue {}

impl SpmcQueue {
    pub fn new(mut size: u64, name: QueueName) -> Self {
        if (size & (size - 1)) != 0 {
            size = round_up_to_pow_2(size)
        }
        Self {
            next_id: UnsafeCell::new(MessageId(0)),
            queue: SpmcFifo::new(size as usize).expect("Failed to create the internal queue"),
            has_producer: AtomicBool::new(false),
            clients: AtomicU64::new(0),
            name,
        }
    }
}

#[inline]
fn round_up_to_pow_2(mut v: u64) -> u64 {
    v -= 1;
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v |= v >> 32;
    v + 1
}

async fn queue_client(log: Logger, stream: warp::ws::WebSocket, exchange: Exchange) {
    info!(log, "Hello client");
    let mut client = crate::client::Client::new(log.clone(), Arc::clone(&exchange));

    async fn _queue_client(
        log_root: Logger,
        stream: warp::ws::WebSocket,
        client: &mut crate::client::Client,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = stream.split();

        let log = log_root.clone();
        while let Some(result) = rx.next().await {
            let msg = match result {
                Ok(m) => m,
                Err(err) => {
                    warn!(log, "Websocket error {:?}", err);
                    break;
                }
            };
            debug!(log, "Handling incoming message");
            if msg.is_binary() || msg.is_text() {
                let cmd: Command = match serde_json::from_slice(msg.as_bytes()) {
                    Ok(m) => m,
                    Err(err) => {
                        warn!(log, "Failed to deserialize message {:?}", err);
                        continue;
                    }
                };
                debug!(log, "Received command {:?}", cmd);
                let res = client
                    .handle_command(log.clone(), cmd)
                    .await
                    .map_err(|err| {
                        debug!(log, "Failed to handle message {:?}", err);
                        err
                    });
                let msg = Message::binary(serde_json::to_vec(&res).unwrap());
                tx.send(msg)
                    .await
                    .with_context(|| "Failed to send response")?;
            }
        }
        Ok(())
    }
    if let Err(err) = _queue_client(log.clone(), stream, &mut client).await {
        warn!(log, "Error running client {:?}", err);
    }
    // cleanup
    match client.queue {
        Some(q) => {
            if client.role.is_producer() {
                q.has_producer.store(false, Ordering::Release);
            }
            let clients = q.clients.fetch_sub(1, Ordering::AcqRel) - 1;
            if clients == 0 && q.queue.is_empty() {
                // this queue can be garbage collected
                exchange.write().unwrap().remove(&q.name);
            }
        }
        _ => {}
    }
}

#[tokio::main]
async fn main() {
    let decorator = slog_term::TermDecorator::new().build();
    let termdrain = slog_term::FullFormat::new(decorator).build().fuse();
    let strategy;
    #[cfg(debug_assertions)]
    {
        strategy = OverflowStrategy::Block;
    }
    #[cfg(not(debug_assertions))]
    {
        strategy = OverflowStrategy::DropAndReport;
    }
    let drain = slog_async::Async::new(termdrain)
        .overflow_strategy(strategy)
        .build()
        .fuse();
    let log = Logger::root(drain, slog::o!());

    // the map might resize when inserting new queues, so put the queues behind pointers
    let exchange: Exchange = Arc::new(RwLock::new(HashMap::new()));

    let exchange = {
        let filter = warp::any().map(move || Arc::clone(&exchange));
        move || filter.clone()
    };

    let log_filter = {
        let log = log.clone();
        let filter = warp::any().map(move || log.clone());
        move || filter.clone()
    };

    let queue_client = warp::get()
        .and(warp::path!("queue-client"))
        .and(warp::ws())
        .and(exchange())
        .and(log_filter())
        .map(|ws: warp::ws::Ws, exchange, log| {
            ws.on_upgrade(move |socket| queue_client(log, socket, exchange))
        });

    let api = queue_client;

    warp::serve(api).run(([127, 0, 0, 1], 6942)).await;
}
