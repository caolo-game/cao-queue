use cao_q_client::{connect, Role};

#[tokio::main]
async fn main() {
    let mut listener_id = 0u32;

    for _ in 0..3 {
        listener_id += 1;
        tokio::task::spawn(async move {
            let mut client = connect("ws://localhost:6942/queue-client").await.unwrap();
            client
                .active_q(Role::Consumer, "myqueue", true)
                .await
                .unwrap()
                .unwrap();
            loop {
                let res = client.listen_for_message().await.unwrap();

                match res.unwrap() {
                    cao_q_client::CommandResponse::Success => {}
                    cao_q_client::CommandResponse::MessageId(_) => {}
                    cao_q_client::CommandResponse::Message(msg) => {
                        println!(
                            "listener ({}) GOT A MSG BOIIIIIIS {:?} payload: {:?}",
                            listener_id,
                            msg.id,
                            String::from_utf8(msg.payload)
                        );
                    }
                };
            }
        });
    }

    let mut client = connect("ws://localhost:6942/queue-client").await.unwrap();
    client
        .active_q(Role::Producer, "myqueue", true)
        .await
        .unwrap()
        .unwrap();

    for _ in 0..100u32 {
        let res = client.push_msg(b"hello bois".to_vec()).await.unwrap();
        println!("push: {:?}", res);
        res.unwrap();
    }

    client.close().await;
}
