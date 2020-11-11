use std::time::Instant;

use caoq_client::{connect, Command, Role};

async fn consumer(url: &'_ str, num_messages: usize, num_threads: usize) {
    let mut client = connect(url).await.unwrap();
    client
        .active_q(Role::Consumer, "myqueue", false)
        .await
        .unwrap()
        .unwrap();

    let limit = num_messages - num_threads;
    let mut has_producer = true;
    let mut has_msg_left = true;

    while has_producer {
        let res = client.listen_for_message(None).await.unwrap();

        match res {
            Ok(caoq_client::CommandResponse::Success) => {}
            Ok(caoq_client::CommandResponse::MessageId(_)) => {}
            Ok(caoq_client::CommandResponse::Message(msg)) => {
                if msg.id.0 as usize > limit {
                    has_msg_left = false;
                    break;
                }
            }
            Err(caoq_client::CommandError::LostProducer) => has_producer = false,
            Err(err) => panic!("{:?}", err),
        };
    }
    // pop the remaining messages if any
    while has_msg_left {
        let res = client.pop_msg().await.unwrap();
        match res.unwrap() {
            caoq_client::CommandResponse::Message(msg) => {
                if msg.id.0 as usize > limit {
                    break;
                }
            }
            _ => break,
        };
    }
    client.close().await
}

pub async fn run(url: &'static str, num_messages: usize, num_threads: usize) {
    let start = Instant::now();

    // make sure we have a producer before we start listening
    let mut client = connect(url).await.unwrap();
    client
        .active_q(Role::Producer, "myqueue", true)
        .await
        .unwrap()
        .unwrap();

    client.send_cmd(Command::ClearQueue).await.unwrap().unwrap();

    // start listeners
    let futures = (0..num_threads)
        .map(|_| tokio::task::spawn(consumer(url, num_messages, num_threads)))
        .collect::<Vec<_>>();

    let b = 69u8;

    // push messages
    for _ in 0..num_messages {
        let msg = vec![b; 512 * 1024];
        let res = client.push_msg(msg).await.unwrap();
        res.unwrap();
    }

    client.close().await;

    // wati for all to retire
    for f in futures {
        f.await.unwrap();
    }

    let end = Instant::now();

    println!("caoq,{:?}", end - start);
}