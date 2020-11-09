use std::{sync::atomic::Ordering, sync::Arc, time::Duration};

use cao_queue::{
    commands::Command, commands::CommandError, commands::CommandResponse, commands::CommandResult,
    message::OwnedMessage, Role,
};
use slog::{debug, trace, warn, Logger};

use crate::{Exchange, SpmcQueue};

/// Local data of a client of the server
pub struct Client {
    pub log: Logger,
    pub role: Role,
    pub queue: Option<Arc<SpmcQueue>>,
    pub exchange: Exchange,
}

impl Client {
    pub fn new(log: Logger, exchange: Exchange) -> Self {
        Self {
            exchange,
            log,
            queue: None,
            role: Role::Consumer, // just as a placeholder, doesn't matter while queue is none
        }
    }

    pub async fn handle_command(&mut self, log: Logger, cmd: Command) -> CommandResult {
        self.log = log.new(slog::o!("role" => format!("{:?}", self.role)));
        match cmd {
            Command::ActiveQueue { role, name, create } => {
                debug!(self.log, "Switching queue");
                let queue = {
                    let mut exchange = self.exchange.write().unwrap();
                    match exchange.entry(name.clone()) {
                        e @ std::collections::hash_map::Entry::Occupied(_) => {
                            Arc::clone(e.or_insert_with(|| unreachable!()))
                        }
                        e @ std::collections::hash_map::Entry::Vacant(_) if create => {
                            debug!(self.log, "Creating queue");
                            // TODO: get size from msg
                            Arc::clone(e.or_insert_with(|| Arc::new(SpmcQueue::new(32000, name))))
                        }
                        _ => {
                            return Err(CommandError::QueueNotFound);
                        }
                    }
                };
                if role.is_producer() {
                    let had_propucer =
                        queue
                            .has_producer
                            .compare_and_swap(false, true, Ordering::Release);
                    if had_propucer {
                        return Err(CommandError::HasProducer);
                    }
                }
                self.role = role;
                self.queue = Some(Arc::clone(&queue));
                Ok(CommandResponse::Success)
            }
            Command::ChangeRole(role) => {
                debug!(self.log, "Changing role");
                if self.queue.is_none() {
                    return Err(CommandError::QueueNotFound);
                }
                if role != self.role {
                    if self.role.is_producer() && !role.is_producer() {
                        self.queue
                            .as_mut()
                            .unwrap()
                            .has_producer
                            .store(false, Ordering::Release);
                    }
                    self.role = role;
                }
                Ok(CommandResponse::Success)
            }
            Command::PushMsg(payload) => {
                debug!(self.log, "Pushing msg");
                if !self.role.is_producer() {
                    return Err(CommandError::NotProducer);
                }
                match self.queue.as_ref() {
                    Some(q) => {
                        let id = unsafe {
                            let id = q.next_id.get();
                            let res = *id;
                            (&mut *id).0 += 1;
                            res
                        };
                        let msg = OwnedMessage { id, payload };
                        match q.queue.push(msg) {
                            Ok(_) => Ok(CommandResponse::MessageId(id)),
                            Err(err) => {
                                warn!(self.log, "Failed to push into queue {}", err);
                                Err(CommandError::QueueError(err))
                            }
                        }
                    }
                    None => Err(CommandError::QueueNotFound),
                }
            }
            Command::PopMsg => {
                debug!(self.log, "Popping msg");
                if !self.role.is_consumer() {
                    return Err(CommandError::NotConsumer);
                }
                match self.queue.as_ref() {
                    Some(q) => match q.queue.pop() {
                        Some(msg) => Ok(CommandResponse::Message(msg)),
                        None => Ok(CommandResponse::Success),
                    },
                    None => Err(CommandError::QueueNotFound),
                }
            }
            Command::ClearQueue => {
                debug!(self.log, "Clearing queue");
                if !self.role.is_producer() {
                    return Err(CommandError::NotProducer);
                }
                let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                q.queue.clear();
                Ok(CommandResponse::Success)
            }
            Command::ListenForMsg => {
                debug!(self.log, "Listening for message");
                if !self.role.is_consumer() {
                    return Err(CommandError::NotConsumer);
                }
                if self.role.is_producer() {
                    // since this is a single-producer queue listening to messages on a producer
                    // _could_ introduce a deadlock and is therefore unallowed
                    return Err(CommandError::WouldBlock);
                }
                let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                // TODO: timeout?
                // TODO config
                let mut sleep_duration = Duration::from_millis(16);
                loop {
                    if let Some(msg) = q.queue.pop() {
                        return Ok(CommandResponse::Message(msg));
                    }
                    if !q.has_producer.load(Ordering::Acquire) {
                        return Err(CommandError::LostProducer);
                    }
                    trace!(
                        self.log,
                        "No messages in the queue, sleeping for {:?}",
                        sleep_duration
                    );
                    tokio::time::delay_for(sleep_duration).await;
                    sleep_duration = sleep_duration.mul_f64(1.2).min(Duration::from_millis(500));
                }
            }
        }
    }
}
