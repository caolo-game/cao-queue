use std::net::TcpStream;

pub use cao_queue::{MessageId, Role};

pub struct Client {
    role: Role,
    socket: websocket::client::r#async::Client<TcpStream>,
}

pub async fn connect_producer(url: &str) -> Client {
    let url = url::Url::parse(url).unwrap(); // TODO
    todo!()
}
