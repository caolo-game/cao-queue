//! Single producer - multi consumer queue clients
//!
#[cfg(test)]
mod tests;

use std::{
    collections::hash_map, collections::HashMap, mem::take, sync::atomic::Ordering, sync::Arc,
    time::Duration, time::Instant,
};

use caoq_core::{
    commands::Command, commands::CommandError, commands::CommandResponse, commands::CommandResult,
    message::OwnedMessage, MessageId, Role,
};
use tracing::{debug, trace, warn};

use crate::{SpmcExchange, SpmcQueue};

/// Local data of a client of the server
pub struct SpmcClient {
    pub role: Role,
    pub queue: Option<Arc<SpmcQueue>>,
    pub exchange: SpmcExchange,
}

impl SpmcClient {
    pub fn new(exchange: SpmcExchange) -> Self {
        Self {
            exchange: exchange.clone(),
            queue: None,
            role: Role::Consumer, // just as a placeholder, doesn't matter while queue is none
        }
    }

    /// clean up after this client
    pub fn cleanup(&mut self) {
        if let Some(q) = self.queue.take() {
            if self.role.is_producer() {
                q.has_producer.store(false, Ordering::Release);
            }
            let clients = q.clients.fetch_sub(1, Ordering::AcqRel) - 1;
            if clients == 0 && q.queue.is_empty() {
                // this queue can be garbage collected
                self.exchange.queues.write().unwrap().remove(&q.name);
            }
        }
    }

    pub async fn handle_command(&mut self, cmd: Command) -> CommandResult {
        match cmd {
            Command::ActiveQueue { role, name, create } => {
                debug!("Switching queue");
                let queue = {
                    let mut exchange = self.exchange.queues.write().unwrap();
                    let q = match exchange.entry(name.clone()) {
                        hash_map::Entry::Occupied(e) => Arc::clone(e.get()),
                        hash_map::Entry::Vacant(e) if create.is_some() => {
                            debug!("Creating queue");
                            let value =
                                Arc::new(SpmcQueue::new(create.unwrap().capacity as u64, name));
                            Arc::clone(e.insert(value))
                        }
                        _ => {
                            return Err(CommandError::QueueNotFound);
                        }
                    };
                    // add 1 to the clients before releasing the lock, so another thread doesn't
                    // "garbage collect" this instance while we're setting it up
                    q.clients.fetch_add(1, Ordering::Release);
                    q
                };
                self.cleanup();
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
                debug!("Changing role");
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
                debug!("Pushing msg");
                if !self.role.is_producer() {
                    return Err(CommandError::NotProducer);
                }
                match self.queue.as_ref() {
                    Some(q) => {
                        let id = unsafe {
                            let id = q.next_id.get();
                            let res = *id;
                            (*id).0 += 1;
                            res
                        };
                        let msg = OwnedMessage { id, payload };
                        match q.queue.push(Arc::new(msg)) {
                            Ok(_) => Ok(CommandResponse::MessageId(id)),
                            Err(err) => {
                                warn!("Failed to push into queue {}", err);
                                Err(CommandError::QueueError(err))
                            }
                        }
                    }
                    None => Err(CommandError::QueueNotFound),
                }
            }
            Command::PopMsg => {
                debug!("Popping msg");
                if !self.role.is_consumer() {
                    return Err(CommandError::NotConsumer);
                }
                let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                match q.queue.pop() {
                    Some(msg) => {
                        let msg = Arc::try_unwrap(msg).unwrap_or_else(|msg| msg.as_ref().clone());
                        Ok(CommandResponse::Message(msg))
                    }
                    None => Ok(CommandResponse::Success),
                }
            }
            Command::ClearQueue => {
                debug!("Clearing queue");
                if !self.role.is_producer() {
                    return Err(CommandError::NotProducer);
                }
                let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                q.queue.clear();
                Ok(CommandResponse::Success)
            }
            Command::ListenForMsg { timeout_ms } => {
                debug!("Listening for message");
                if !self.role.is_consumer() {
                    return Err(CommandError::NotConsumer);
                }
                if self.role.is_producer() {
                    // since this is a single-producer queue listening to messages on a producer
                    // _could_ introduce a deadlock and is therefore unallowed
                    return Err(CommandError::WouldBlock);
                }
                let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                let mut sleep_duration = Duration::from_millis(16); // TODO config
                let mut total = Duration::from_millis(0);
                let timeout = timeout_ms.map(Duration::from_millis);
                'retry: loop {
                    if let Some(msg) = q.queue.pop() {
                        let msg = Arc::try_unwrap(msg).unwrap_or_else(|msg| msg.as_ref().clone());
                        return Ok(CommandResponse::Message(msg));
                    }
                    if !q.has_producer.load(Ordering::Acquire) {
                        return Err(CommandError::LostProducer);
                    }
                    trace!(
                        "No messages in the queue, sleeping for {:?}",
                        sleep_duration
                    );
                    let start = Instant::now();
                    tokio::time::delay_for(sleep_duration).await;
                    if let Some(timeout) = timeout.as_ref() {
                        total += Instant::now() - start; // note that this isn't necessarily equal to `sleep_duration`
                        if &total >= timeout {
                            break 'retry Err(CommandError::Timeout);
                        }
                    }
                    sleep_duration = sleep_duration.mul_f64(1.2).min(Duration::from_millis(500));
                }
            }
            Command::MsgResponse { id, payload } => {
                let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                let mut l = q.responses.lock();
                l.insert(id, payload);
                Ok(CommandResponse::Success)
            }
            Command::ConsumeResponses => {
                if !self.role.is_producer() {
                    return Err(CommandError::NotProducer);
                }
                let responses: HashMap<MessageId, Vec<u8>> = {
                    let q = self.queue.as_ref().ok_or(CommandError::QueueNotFound)?;
                    let mut l = q.responses.lock();
                    take(&mut *l)
                };

                let response = responses
                    .into_iter()
                    .map(|(id, payload)| OwnedMessage { id, payload })
                    .collect();

                Ok(CommandResponse::Messages(response))
            }
        }
    }
}
