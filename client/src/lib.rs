pub use caoq_core::{commands::*, message::OwnedMessage};
pub use caoq_core::{MessageId, Role};
use futures_util::sink::SinkExt;
use tokio::{net::TcpStream, stream::StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

type Ws = tokio_tungstenite::WebSocketStream<TcpStream>;

pub struct Client {
    socket: Ws,
}

impl Client {
    pub async fn send_cmd(&mut self, cmd: Command) -> Result<CommandResponse, CaoQError> {
        let payload = bincode::serialize(&cmd).unwrap();
        let msg = Message::binary(payload);

        self.socket.send(msg).await.unwrap();
        let resp: CommandResponse = loop {
            let msg = self.socket.next().await;
            if let Some(msg) = msg {
                match msg.map_err(ClientError::WebsocketError)? {
                    msg @ Message::Text(_) | msg @ Message::Binary(_) => {
                        break bincode::deserialize::<CommandResult>(msg.into_data().as_slice())
                            .unwrap()?
                    }
                    Message::Ping(_) => {
                        self.socket
                            .send(Message::Pong(vec![]))
                            .await
                            .map_err(ClientError::WebsocketError)?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(msg) => {
                        return Err(ClientError::ConnectionClosed(
                            msg.map(|msg| format!("{:?}", msg)),
                        ))?
                    }
                }
            }
        };
        Ok(resp)
    }

    pub async fn push_msg(&mut self, payload: Vec<u8>) -> Result<MessageId, CaoQError> {
        let cmd = Command::PushMsg(payload);
        self.send_cmd(cmd).await.map(|res| match res {
            CommandResponse::MessageId(id) => id,
            CommandResponse::Success
            | CommandResponse::Message(_)
            | CommandResponse::Messages(_) => {
                unreachable!()
            }
        })
    }

    pub async fn change_role(&mut self, role: Role) -> Result<(), CaoQError> {
        let cmd = Command::ChangeRole(role);
        self.send_cmd(cmd).await.map(|res| {
            debug_assert!(matches!(res, CommandResponse::Success));
        })
    }

    pub async fn active_queue(
        &mut self,
        role: Role,
        name: String,
        create: Option<QueueOptions>,
    ) -> Result<(), CaoQError> {
        let cmd = Command::ActiveQueue { role, name, create };
        self.send_cmd(cmd).await.map(|res| {
            debug_assert!(matches!(res, CommandResponse::Success));
        })
    }

    pub async fn pop_msg(&mut self) -> Result<Option<OwnedMessage>, CaoQError> {
        self.send_cmd(Command::PopMsg).await.map(|res| match res {
            CommandResponse::Success => None,
            CommandResponse::Message(msg) => Some(msg),
            CommandResponse::MessageId(_) | CommandResponse::Messages(_) => {
                unreachable!()
            }
        })
    }

    pub async fn msg_response(&mut self, id: MessageId, payload: Vec<u8>) -> Result<(), CaoQError> {
        self.send_cmd(Command::MsgResponse { id, payload })
            .await
            .map(|res| {
                debug_assert!(matches!(res, CommandResponse::Success));
            })
    }

    pub async fn listen_for_msg(
        &mut self,
        timeout_ms: Option<u64>,
    ) -> Result<OwnedMessage, CaoQError> {
        self.send_cmd(Command::ListenForMsg { timeout_ms })
            .await
            .map(|res| match res {
                CommandResponse::Message(msg) => msg,
                CommandResponse::Success
                | CommandResponse::MessageId(_)
                | CommandResponse::Messages(_) => {
                    unreachable!()
                }
            })
    }

    pub async fn clear_queue(&mut self) -> Result<(), CaoQError> {
        self.send_cmd(Command::ClearQueue).await.map(|res| {
            debug_assert!(matches!(res, CommandResponse::Success));
        })
    }

    pub async fn consume_responses(&mut self) -> Result<Vec<OwnedMessage>, CaoQError> {
        self.send_cmd(Command::ConsumeResponses)
            .await
            .map(|res| match res {
                CommandResponse::Messages(msgs) => msgs,
                CommandResponse::Success
                | CommandResponse::MessageId(_)
                | CommandResponse::Message(_) => {
                    unreachable!()
                }
            })
    }

    pub async fn close(&mut self) {
        self.socket.close(None).await.unwrap_or(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CaoQError {
    #[error("Failure in the client {0}")]
    ClientError(ClientError),
    #[error("Faild to execute the command {0}")]
    CommandError(CommandError),
}

impl From<ClientError> for CaoQError {
    fn from(err: ClientError) -> Self {
        CaoQError::ClientError(err)
    }
}

impl From<CommandError> for CaoQError {
    fn from(err: CommandError) -> Self {
        CaoQError::CommandError(err)
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
