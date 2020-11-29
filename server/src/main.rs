mod client;

use anyhow::Context;
use client::{spmc::SpmcClient, QueueClient};
use futures_util::{SinkExt, StreamExt};
use std::{
    cell::UnsafeCell, collections::HashMap, env, net::IpAddr, sync::atomic::AtomicBool,
    sync::atomic::AtomicU64, sync::Arc, sync::RwLock,
};
use tracing_subscriber::fmt::format::FmtSpan;
use warp::{ws::Message, Filter};

use caoq_core::{
    collections::spmcbounded::SpmcBounded, commands::Command, message::OwnedMessage, MessageId,
};
use parking_lot::Mutex;
use tracing::{debug, error, info, warn};

type QueueName = String; // TODO: short string?

#[derive(Clone)]
pub struct SpmcExchange {
    pub queues: Arc<RwLock<HashMap<QueueName, Arc<SpmcQueue>>>>,
}

/// Single producer - multi consumer queue
pub struct SpmcQueue {
    pub next_id: UnsafeCell<MessageId>,
    pub queue: SpmcBounded<Arc<OwnedMessage>>,
    pub has_producer: AtomicBool,
    /// number of connected, active clients
    pub clients: AtomicU64,
    pub name: QueueName,
    pub responses: Arc<Mutex<HashMap<MessageId, Vec<u8>>>>,
}

unsafe impl<'a> Sync for SpmcQueue {}

impl SpmcQueue {
    pub fn new(mut size: u64, name: QueueName) -> Self {
        if (size & (size - 1)) != 0 {
            size = round_up_to_pow_2(size)
        }
        Self {
            next_id: UnsafeCell::new(MessageId(0)),
            queue: SpmcBounded::new(size as usize).expect("Failed to create the internal queue"),
            has_producer: AtomicBool::new(false),
            clients: AtomicU64::new(0),
            responses: Arc::new(Mutex::new(HashMap::new())),
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

async fn run_queue_client(stream: warp::ws::WebSocket, mut client: QueueClient) {
    info!("Hello client");

    async fn _queue_client(
        stream: warp::ws::WebSocket,
        client: &mut QueueClient,
    ) -> anyhow::Result<()> {
        let (mut tx, mut rx) = stream.split();

        while let Some(result) = rx.next().await {
            let msg = match result {
                Ok(m) => m,
                Err(err) => {
                    warn!("Websocket error {:?}", err);
                    break;
                }
            };
            debug!("Handling incoming message");
            if msg.is_binary() || msg.is_text() {
                let cmd: Command = match bincode::deserialize(msg.as_bytes()) {
                    Ok(m) => m,
                    Err(err) => {
                        warn!("Failed to deserialize message {:?}", err);
                        continue;
                    }
                };
                debug!("Received command {:?}", cmd);
                let res = client.handle_command(cmd).await.map_err(|err| {
                    debug!("Failed to handle message {:?}", err);
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
    if let Err(err) = _queue_client(stream, &mut client).await {
        warn!("Error running client {:?}", err);
    }
    client.cleanup();

    info!("Bye client");
}

async fn spmc_queue(stream: warp::ws::WebSocket, exchange: SpmcExchange) {
    let client = SpmcClient::new(exchange);
    run_queue_client(stream, QueueClient::Spmc(client)).await
}

#[tokio::main]
async fn main() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned());
    // Configure the default `tracing` subscriber.
    // The `fmt` subscriber from the `tracing-subscriber` crate logs `tracing`
    // events to stdout. Other subscribers are available for integrating with
    // distributed tracing systems such as OpenTelemetry.
    tracing_subscriber::fmt()
        // Use the filter we built above to determine which traces to record.
        .with_env_filter(filter)
        // Record an event when each span closes. This can be used to time our
        // routes' durations!
        .with_span_events(FmtSpan::CLOSE)
        .init();

    // the map might resize when inserting new queues, so put the queues behind pointers
    let exchange = SpmcExchange {
        queues: Arc::new(RwLock::new(HashMap::new())),
    };

    let exchange = {
        let filter = warp::any().map(move || exchange.clone());
        move || filter.clone()
    };

    let spmc_queue = warp::get()
        .and(warp::path!("spmc"))
        .and(warp::ws())
        .and(exchange())
        .map(|ws: warp::ws::Ws, exchange| {
            ws.on_upgrade(move |socket| spmc_queue(socket, exchange))
        });

    let health = warp::get().and(warp::path("health")).map(warp::reply);

    let api = spmc_queue.or(health).with(warp::trace::request());

    let host: IpAddr = env::var("HOST")
        .ok()
        .and_then(|host| {
            host.parse()
                .map_err(|e| {
                    error!("Failed to parse host {:?}", e);
                })
                .ok()
        })
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]));

    let port = env::var("PORT")
        .map_err(anyhow::Error::new)
        .and_then(|port| port.parse().map_err(anyhow::Error::new))
        .unwrap_or_else(|err| {
            warn!("Failed to parse port number: {}", err);
            6942
        });

    info!("Starting service on {:?}:{:?}", host, port);

    warp::serve(api).run((host, port)).await;
}
