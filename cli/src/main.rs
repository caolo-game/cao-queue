use caoq_client::{connect, Role};

#[tokio::main]
async fn main() {
    for listener_id in 0..3u32 {
        tokio::task::spawn(async move {
            let mut client = connect("ws://localhost:6942/spmc-queue-client")
                .await
                .unwrap();
            client
                .active_q(Role::Consumer, "myqueue", true)
                .await
                .unwrap()
                .unwrap();
            loop {
                let res = client.listen_for_message(None).await.unwrap();

                match res.unwrap() {
                    caoq_client::CommandResponse::Success => {}
                    caoq_client::CommandResponse::MessageId(_) => {}
                    caoq_client::CommandResponse::Message(msg) => {
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

    let mut client = connect("ws://localhost:6942/spmc-queue-client")
        .await
        .unwrap();
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
