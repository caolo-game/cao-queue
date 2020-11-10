use std::sync::{Once, RwLock};
static INIT: Once = Once::new();

pub fn setup_testing() {
    INIT.call_once(|| {
        env_logger::init();
    });
}

use slog::{o, Drain};

use super::*;

fn test_logger() -> slog::Logger {
    Logger::root(slog_stdlog::StdLog.fuse(), o!())
}

fn setup_client() -> SpmcClient {
    let exchange = Arc::new(RwLock::new(Default::default()));

    SpmcClient::new(test_logger(), exchange)
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
            create: false,
        };

        // note: in practice don't pass the same logger to the function as it override the internal one...
        let res = client.handle_command(client.log.clone(), cmd).await;
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
            create: true,
        };

        // note: in practice don't pass the same logger to the function as it override the internal one...
        let res = client.handle_command(client.log.clone(), cmd).await;
        assert!(matches!(res.unwrap(), CommandResponse::Success));

        assert!(client.exchange.read().unwrap().contains_key("asd"));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn fails_if_already_has_producer() {
        setup_testing();

        let mut client = setup_client();

        let cmd = Command::ActiveQueue {
            role: Role::Producer,
            name: "asd".to_owned(),
            create: true,
        };

        {
            let mut exch = client.exchange.write().unwrap();
            exch.insert("asd".into(), Arc::new(SpmcQueue::new(128, "asd".into())));
            exch["asd"].has_producer.store(true, Ordering::Release);
        }

        // note: in practice don't pass the same logger to the function as it override the internal one...
        let res = client.handle_command(client.log.clone(), cmd).await;
        assert!(matches!(res.unwrap_err(), CommandError::HasProducer));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn registers_client() {
        setup_testing();

        let mut client = setup_client();

        let cmd = Command::ActiveQueue {
            role: Role::Producer,
            name: "asd".to_owned(),
            create: true,
        };

        let res = client.handle_command(client.log.clone(), cmd).await;
        assert!(matches!(res.unwrap(), CommandResponse::Success));

        assert!(
            client
                .exchange
                .read()
                .unwrap()
                .get("asd")
                .expect("expected to find the queue")
                .clients
                .load(Ordering::Acquire)
                == 1
        );
    }
}
