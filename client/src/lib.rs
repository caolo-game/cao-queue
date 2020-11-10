pub use cao_queue::{commands::*, message::OwnedMessage};
pub use cao_queue::{MessageId, Role};
use futures_util::sink::SinkExt;
use tokio::{net::TcpStream, stream::StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

type Ws = tokio_tungstenite::WebSocketStream<TcpStream>;

pub struct Client {
    role: Role,
    socket: Ws,
}

impl Client {
    pub async fn send_cmd(&mut self, cmd: Command) -> Result<CommandResult, ClientError> {
        let payload = serde_json::to_vec(&cmd).unwrap();

        let msg = Message::binary(payload);

        self.socket.send(msg).await.unwrap();

        let resp: CommandResult = loop {
            let msg = self.socket.next().await;
            if let Some(msg) = msg {
                match msg? {
                    msg @ Message::Text(_) | msg @ Message::Binary(_) => {
                        break serde_json::from_slice(msg.into_data().as_slice()).unwrap()
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

    pub async fn active_q(
        &mut self,
        role: Role,
        name: &str,
        create: bool,
    ) -> Result<CommandResult, ClientError> {
        let cmd = Command::ActiveQueue {
            role,
            name: name.to_owned(),
            create,
        };
        let res = self.send_cmd(cmd).await?;
        self.role = role;
        Ok(res)
    }

    pub async fn push_msg(&mut self, msg: Vec<u8>) -> Result<CommandResult, ClientError> {
        let cmd = Command::PushMsg(msg);
        self.send_cmd(cmd).await
    }

    pub async fn pop_msg(&mut self) -> Result<CommandResult, ClientError> {
        let cmd = Command::PopMsg;
        self.send_cmd(cmd).await
    }

    pub async fn listen_for_message(
        &mut self,
        timeout_ms: Option<u64>,
    ) -> Result<CommandResult, ClientError> {
        let cmd = Command::ListenForMsg { timeout_ms };
        self.send_cmd(cmd).await
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

    let client = Client {
        role: Role::NoRole,
        socket,
    };
    Ok(client)
}
