use std::collections::HashMap;

use cao_queue::{collections::spmcfifo::SpmcFifo, MessageId};

pub struct Queue {
    pub name: String,
    pub next_id: MessageId,
    pub queue: SpmcFifo,
    pub messages: HashMap<MessageId, Vec<u8>>,
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");
}
