use std::{
    cell::Cell, cell::UnsafeCell, collections::HashMap, mem::size_of, pin::Pin, sync::Arc,
    sync::RwLock,
};

use cao_queue::{
    collections::bucket_allocator::BucketAllocator, collections::spmcfifo::SpmcFifo,
    message::ManagedMessage, MessageId,
};
use slog::{info, Drain, Logger};
use warp::{hyper::StatusCode, ws::WebSocket, Filter};

type Queues<'a> = Arc<RwLock<HashMap<String, Arc<SpmcQueue<'a>>>>>;

/// Single producer - multi consumer queue
pub struct SpmcQueue<'a> {
    pub next_id: UnsafeCell<MessageId>,
    pub queue: SpmcFifo<ManagedMessage<'a>>,
    pub allocator: Pin<Box<BucketAllocator>>,
}

unsafe impl<'a> Sync for SpmcQueue<'a> {}

impl<'a> SpmcQueue<'a> {
    pub fn new(mut size: u64) -> Self {
        if (size & (size - 1)) != 0 {
            size = round_up_to_pow_2(size)
        }
        Self {
            next_id: UnsafeCell::new(MessageId(0)),
            // q takes size in number of elements
            queue: SpmcFifo::new(size as usize).expect("Failed to create the internal queue"),
            // allocator takes size in bytes
            allocator: BucketAllocator::new(size_of::<ManagedMessage>() * size as usize),
        }
    }
}

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

#[derive(serde::Deserialize)]
pub struct QueueDeclare {
    pub name: String,
    pub size: Option<u64>,
}

async fn producer(log: Logger, socket: WebSocket, queue: Arc<SpmcQueue<'_>>) {
    info!(log, "Hello, producer");
}

#[tokio::main]
async fn main() {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let log = Logger::root(drain, slog::o!());

    let _scope_guard = slog_scope::set_global_logger(log.clone());
    let _log_guard = slog_stdlog::init().unwrap();

    // the map might resize when inserting new queues, so put the queues behind pointers
    let queues: Queues = Arc::new(RwLock::new(HashMap::<String, Arc<SpmcQueue<'_>>>::new()));

    let queues = {
        let queues = Arc::clone(&queues);
        let filter = warp::any().map(move || Arc::clone(&queues));
        move || filter.clone()
    };

    let log = {
        let filter = warp::any().map(move || log.clone());
        move || filter.clone()
    };

    let producer = warp::get()
        .and(warp::path!("queue" / String / "producer"))
        .and(log())
        .and(warp::ws())
        .and(queues())
        .map(
            |qname: String, log: Logger, ws: warp::ws::Ws, qs: Queues| -> Box<dyn warp::Reply> {
                let qs = qs.read().unwrap();
                let q = qs.get(&qname);
                let q = unsafe { std::mem::transmute(q) };
                let log = log.new(slog::o!("queue" => qname, "role" => "producer"));
                match q {
                    Some(q) => {
                        Box::new(ws.on_upgrade(move |socket| producer(log, socket, Arc::clone(q))))
                    }
                    None => Box::new(warp::reply::with_status(
                        warp::reply(),
                        StatusCode::NOT_FOUND,
                    )),
                }
            },
        );

    let qdeclare = warp::post()
        .and(warp::path!("queue"))
        .and(queues())
        .and(warp::body::json())
        .map(|qs: Queues, QueueDeclare { name, size }| {
            let mut qs = qs.write().unwrap();
            qs.entry(name)
                .or_insert_with(|| Arc::new(SpmcQueue::new(size.unwrap_or(1024))));
            warp::reply()
        });

    let api = producer.or(qdeclare);

    warp::serve(api).run(([127, 0, 0, 1], 6942)).await;
}
