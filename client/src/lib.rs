pub use cao_queue::{commands::*, message::OwnedMessage};
pub use cao_queue::{MessageId, Role};
use futures_util::sink::SinkExt;
use tokio::{net::TcpStream, stream::StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

type Ws = tokio_tungstenite::WebSocketStream<TcpStream>;

pub struct Client {
    socket: Ws,
}

impl Client {
    pub async fn send_cmd(&mut self, cmd: Command) -> Result<CommandResult, ClientError> {
        let payload = bincode::serialize(&cmd).unwrap();
        let msg = Message::binary(payload);

        self.socket.send(msg).await.unwrap();
        let resp: CommandResult = loop {
            let msg = self.socket.next().await;
            if let Some(msg) = msg {
                match msg? {
                    msg @ Message::Text(_) | msg @ Message::Binary(_) => {
                        break bincode::deserialize(msg.into_data().as_slice()).unwrap()
                    }
                    Message::Ping(_) => {
                        self.socket.send(Message::Pong(vec![])).await?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(msg) => {
                        return Err(ClientError::ConnectionClosed(
                            msg.map(|msg| format!("{:?}", msg)),
                        ))
                    }
                }
            }
        };
        Ok(resp)
    }

    pub async fn close(&mut self) {
        self.socket.close(None).await.unwrap_or(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Failed to parse url {0}")]
    BadUrl(url::ParseError),
    #[error("Websocket error: {0}")]
    WebsocketError(tokio_tungstenite::tungstenite::Error),
    #[error("Websocket connection closed {0:?}")]
    ConnectionClosed(Option<String>),
}

impl From<tokio_tungstenite::tungstenite::Error> for ClientError {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        ClientError::WebsocketError(err)
    }
}

pub async fn connect(url: &str) -> Result<Client, ClientError> {
    let (socket, _) = connect_async(Url::parse(url).map_err(ClientError::BadUrl)?).await?;

    let client = Client { socket };
    Ok(client)
}
