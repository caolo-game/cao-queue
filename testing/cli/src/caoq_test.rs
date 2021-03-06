use std::time::Instant;

use caoq_client::{connect, CaoQError, Command, CommandError, CommandResponse, QueueOptions, Role};

async fn consumer(url: &'_ str, num_messages: usize, num_threads: usize) {
    let mut client = connect(url).await.unwrap();
    client
        .send_cmd(Command::ActiveQueue {
            role: Role::Consumer,
            name: "myqueue".into(),
            create: None,
        })
        .await
        .unwrap();

    let limit = num_messages - num_threads - 1;
    let mut has_producer = true;
    let mut has_msg_left = true;

    while has_producer {
        let res = client
            .send_cmd(Command::ListenForMsg { timeout_ms: None })
            .await;

        match res {
            Ok(CommandResponse::Success) => {}
            Ok(CommandResponse::MessageId(_)) => {}
            Ok(CommandResponse::Message(msg)) => {
                if msg.id.0 as usize > limit {
                    has_msg_left = false;
                    break;
                }
            }
            Ok(CommandResponse::Messages(msgs)) => {
                for msg in msgs {
                    if msg.id.0 as usize > limit {
                        has_msg_left = false;
                        break;
                    }
                }
            }
            Err(CaoQError::CommandError(CommandError::LostProducer)) => has_producer = false,
            Err(err) => panic!("{:?}", err),
        };
    }
    // pop the remaining messages if any
    while has_msg_left {
        let res = client.send_cmd(Command::PopMsg).await.unwrap();
        match res {
            CommandResponse::Message(msg) => {
                if msg.id.0 as usize > limit {
                    has_msg_left = false;
                }
            }
            _ => has_msg_left = false,
        };
    }
    client.close().await
}

pub async fn run(url: &'static str, num_messages: usize, num_threads: usize) {
    let start = Instant::now();

    // make sure we have a producer before we start listening
    let mut client = connect(url).await.unwrap();
    client
        .send_cmd(Command::ActiveQueue {
            role: Role::Producer,
            name: "myqueue".into(),
            create: Some(QueueOptions { capacity: 16_000 }),
        })
        .await
        .unwrap();

    client.send_cmd(Command::ClearQueue).await.unwrap();

    // start listeners
    let futures = (0..num_threads)
        .map(|_| tokio::task::spawn(consumer(url, num_messages, num_threads)))
        .collect::<Vec<_>>();

    let b = 69u8;

    // push messages
    for _ in 0..num_messages {
        let msg = vec![b; 512 * 1024];
        let _ = client.send_cmd(Command::PushMsg(msg)).await.unwrap();
    }

    client.close().await;

    // wati for all to retire
    for f in futures {
        f.await.unwrap();
    }

    let end = Instant::now();

    println!("caoq,{:?}", end - start);
}
