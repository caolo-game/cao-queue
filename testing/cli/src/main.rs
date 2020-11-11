use clap::{App, Arg};

#[cfg(feature = "caoq-client")]
mod caoq_test;

#[tokio::main]
async fn main() {
    let matches = App::new("allocation benchmark sample")
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
        .get_matches();

    let samples: usize = matches
        .value_of("samples")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let messages: usize = matches
        .value_of("messages")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);

    for _ in 0..samples {
        #[cfg(feature = "caoq-client")]
        crate::caoq_test::run("ws://localhost:6942/spmc-queue-client", messages, 2).await;
    }
}
