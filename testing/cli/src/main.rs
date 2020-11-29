use clap::{App, Arg};

mod caoq_test;
mod redis_test;

#[tokio::main]
async fn main() {
    let matches = App::new("message queues benchmark")
        .arg(
            Arg::with_name("samples")
                .short("s")
                .long("samples")
                .value_name("SAMPLES")
                .help("Number of iterations to run")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("messages")
                .short("m")
                .long("messages")
                .value_name("MESSAGES")
                .help("Number of messages to send")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("queue")
                .short("q")
                .long("queue")
                .value_name("QUEUE")
                .help("Name of the queue to use, one of [caoq,redis,rabbit]")
                .takes_value(true),
        )
        .get_matches();

    let samples: usize = matches
        .value_of("samples")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let messages: usize = matches
        .value_of("messages")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    match matches.value_of("queue").unwrap_or("caoq") {
        "caoq" => {
            async move {
                for _ in 0..samples {
                    crate::caoq_test::run("ws://localhost:6942/spmc", messages, 2).await;
                }
            }
            .await
        }
        "redis" => {
            async move {
                for _ in 0..samples {
                    crate::redis_test::run("redis://localhost:6379/0", messages, 2).await;
                }
            }
            .await
        }
        q @ _ => unimplemented!("Queue type ({}) isn't implemented", q),
    }
}
