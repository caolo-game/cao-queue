use std::sync::{Once, RwLock};
static INIT: Once = Once::new();

pub fn setup_testing() {
    INIT.call_once(|| {
        env_logger::init();
    });
}

use caoq_core::commands::QueueOptions;

use super::*;

fn setup_client() -> QueueClient {
    let exchange = crate::BoundedExchange {
        queues: Arc::new(RwLock::new(Default::default())),
    };
    QueueClient::new(Uuid::new_v4(), exchange)
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn change_q_cleans_up() {
    setup_testing();

    let mut client = setup_client();
    client
        .handle_command(Command::ActiveQueue {
            role: Role::Producer,
            name: "boi".to_owned(),
            create: Some(QueueOptions { capacity: 8000 }),
        })
        .await
        .unwrap();

    let q = Arc::clone(client.queue.as_ref().unwrap());
    assert_eq!(q.clients.load(Ordering::Relaxed), 1);

    client
        .handle_command(Command::ActiveQueue {
            role: Role::Producer,
            name: "boi2".to_owned(),
            create: Some(QueueOptions { capacity: 8000 }),
        })
        .await
        .unwrap();

    assert_eq!(q.clients.load(Ordering::Relaxed), 0);
    let q = Arc::clone(client.queue.as_ref().unwrap());
    assert_eq!(q.name, "boi2");
    assert_eq!(q.clients.load(Ordering::Relaxed), 1);
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn clear_fails_if_not_producer() {
    setup_testing();

    let mut client = setup_client();
    client.role = Role::Consumer;
    let cmd = Command::ClearQueue;

    let err = client
        .handle_command(cmd)
        .await
        .expect_err("Expected clear to fail");
    assert!(matches!(err, CommandError::NotProducer));
}

mod active_queue_command {
    use super::*;

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn fails_if_queue_not_exists() {
        setup_testing();

        let mut client = setup_client();

        let cmd = Command::ActiveQueue {
            role: Role::NoRole,
            name: "asd".to_owned(),
            create: None, // <-- important
        };

        // note: in practice don't pass the same logger to the function as it override the internal one...
        let res = client.handle_command(cmd).await;
        assert!(matches!(res.unwrap_err(), CommandError::QueueNotFound));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn creates_the_q_if_not_exists() {
        setup_testing();

        let mut client = setup_client();

        let cmd = Command::ActiveQueue {
            role: Role::NoRole,
            name: "asd".to_owned(),
            create: Some(QueueOptions { capacity: 8000 }),
        };

        // note: in practice don't pass the same logger to the function as it override the internal one...
        let res = client.handle_command(cmd).await;
        assert!(matches!(res.unwrap(), CommandResponse::Success));

        assert!(client.exchange.queues.read().unwrap().contains_key("asd"));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn registers_client() {
        setup_testing();

        let mut client = setup_client();

        let cmd = Command::ActiveQueue {
            role: Role::Producer,
            name: "asd".to_owned(),
            create: Some(QueueOptions { capacity: 8000 }),
        };

        let res = client.handle_command(cmd).await;
        assert!(matches!(res.unwrap(), CommandResponse::Success));

        assert!(
            client
                .exchange
                .queues
                .read()
                .unwrap()
                .get("asd")
                .expect("expected to find the queue")
                .clients
                .load(Ordering::Relaxed)
                == 1
        );
    }
}
