mod client;

use anyhow::Context;
use client::spmc::SpmcClient;
use futures_util::{SinkExt, StreamExt};
use slog_async::OverflowStrategy;
use std::{
    cell::UnsafeCell, collections::HashMap, env, net::IpAddr, sync::atomic::AtomicBool,
    sync::atomic::AtomicU64, sync::Arc, sync::RwLock,
};
use warp::{ws::Message, Filter};

use cao_queue::{
    collections::spmcfifo::SpmcFifo, commands::Command, message::OwnedMessage, MessageId,
};
use slog::{debug, error, info, warn, Drain, Logger};

/// Collection of queues by name
type SpmcExchange = Arc<RwLock<HashMap<QueueName, Arc<SpmcQueue>>>>;
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

async fn queue_client(log: Logger, stream: warp::ws::WebSocket, exchange: SpmcExchange) {
    info!(log, "Hello client");
    let mut client = SpmcClient::new(log.clone(), Arc::clone(&exchange));

    async fn _queue_client(
        log_root: Logger,
        stream: warp::ws::WebSocket,
        client: &mut SpmcClient,
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
                let cmd: Command = match bincode::deserialize(msg.as_bytes()) {
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
                let msg = Message::binary(bincode::serialize(&res).unwrap());
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
    client.cleanup();
    info!(log, "Bye client");
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
    let exchange: SpmcExchange = Arc::new(RwLock::new(HashMap::new()));

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
        .and(warp::path!("spmc-queue-client"))
        .and(warp::ws())
        .and(exchange())
        .and(log_filter())
        .map(|ws: warp::ws::Ws, exchange, log| {
            ws.on_upgrade(move |socket| queue_client(log, socket, exchange))
        });

    let health = warp::get().and(warp::path("health")).map(|| warp::reply());

    let api = queue_client.or(health);

    let host: IpAddr = env::var("HOST")
        .ok()
        .and_then(|host| {
            host.parse()
                .map_err(|e| {
                    error!(log, "Failed to parse host {:?}", e);
                })
                .ok()
        })
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]));

    let port = env::var("PORT")
        .map_err(anyhow::Error::new)
        .and_then(|port| port.parse().map_err(anyhow::Error::new))
        .unwrap_or_else(|err| {
            warn!(log, "Failed to parse port number: {}", err);
            6942
        });

    info!(log, "Starting service on {:?}:{:?}", host, port);

    warp::serve(api).run((host, port)).await;
}
