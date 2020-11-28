use caoq_client::{MessageId, OwnedMessage};
use redis::AsyncCommands;
use std::time::Instant;

async fn consumer(url: &'_ str, num_messages: usize, num_threads: usize) {
    let client = redis::Client::open(url).expect("open conn");
    let mut conn = client.get_async_connection().await.expect("get conn");

    let limit = num_messages - num_threads - 1;

    loop {
        let msg: Option<Vec<u8>> = conn.lpop("myqueue").await.expect("pop");
        if let Some(msg) = msg {
            let msg: OwnedMessage = bincode::deserialize(msg.as_slice()).expect("deserialize");
            if msg.id.0 as usize > limit {
                // done
                return;
            }
        }
    }
}

pub async fn run(url: &'static str, num_messages: usize, num_threads: usize) {
    let start = Instant::now();

    let client = redis::Client::open(url).expect("open conn");
    let mut conn = client.get_async_connection().await.expect("get conn");
    // reset the queue, in case there's junk from previous tests
    let _: () = conn.del("myqueue").await.expect("reset failed");

    // start listeners
    let futures = (0..num_threads)
        .map(|_| tokio::task::spawn(consumer(url, num_messages, num_threads)))
        .collect::<Vec<_>>();

    let b = 69u8;

    // push messages
    for id in 0..num_messages as u64 {
        let msg = vec![b; 512 * 1024];
        let msg = OwnedMessage {
            id: MessageId(id),
            payload: msg,
        };
        let payload = bincode::serialize(&msg).expect("ser");
        let _: () = conn.rpush("myqueue", payload).await.unwrap();
    }

    // wati for all to retire
    for f in futures {
        f.await.unwrap();
    }

    let end = Instant::now();

    println!("redis,{:?}", end - start);
}
