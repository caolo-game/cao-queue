use super::{spmcbounded::SpmcBounded, QueueError};
use std::{collections::HashMap, pin::Pin, sync::Arc, sync::Mutex};

use left_right::Absorb;
use uuid::Uuid;

// when removing a queue other threads may still be reading from it so we'll use Arc instead of Box
pub type Inner<T> = Pin<Arc<SpmcBounded<T>>>;

#[derive(Clone)]
struct Storage<T>(HashMap<Uuid, Inner<T>>);

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Storage(HashMap::new())
    }
}

enum QueueOp<T> {
    AddQueue(Uuid, Inner<T>),
    RemoveQueue(Uuid),
}

impl<T> Absorb<QueueOp<T>> for Storage<T> {
    fn absorb_first(&mut self, operation: &mut QueueOp<T>, _: &Self) {
        match operation {
            QueueOp::AddQueue(id, q) => {
                self.0.insert(*id, q.clone());
            }
            QueueOp::RemoveQueue(id) => {
                self.0.remove(&id);
            }
        }
    }
}

pub struct BoundedQueue<T> {
    writer: Mutex<left_right::WriteHandle<Storage<T>, QueueOp<T>>>,
    reader: left_right::ReadHandle<Storage<T>>,
    default_capacity: usize,
}

impl<T> BoundedQueue<T>
where
    T: Unpin + Clone,
{
    pub fn new(capacity: usize) -> Self {
        let (writer, reader) = left_right::new::<Storage<T>, QueueOp<T>>();
        let writer = Mutex::new(writer);
        Self {
            writer,
            reader,
            default_capacity: capacity,
        }
    }

    pub fn is_empty(&self) -> bool {
        let reader = self.reader.enter().unwrap();
        reader.0.iter().all(|(_, q)| q.is_empty())
    }

    /// Lazily initilizes the producers if needed
    pub fn push(&self, id: Uuid, item: T) -> Result<(), QueueError> {
        // if this queue already exists just push the item
        if let Some(q) = self.reader.enter().unwrap().0.get(&id) {
            return q.push(item);
        }
        // create the new queue and push the item
        let q = SpmcBounded::new(self.default_capacity)?;
        q.push(item)?;

        let q = Arc::pin(q);

        // insert our new queue into the backend
        let mut writer = self.writer.lock().unwrap();
        writer.append(QueueOp::AddQueue(id, q));
        writer.publish();
        Ok(())
    }

    /// This method is intended to be used when you want to use custom queues for given producers.
    /// For the default behaviour just skip this and use `push` directly.
    pub fn register_producer(&self, id: Uuid, queue: Inner<T>) -> Result<(), QueueError> {
        let mut writer = self.writer.lock().unwrap();
        writer.append(QueueOp::AddQueue(id, queue));
        writer.publish();
        Ok(())
    }

    pub fn remove_producer(&self, id: Uuid) {
        let mut writer = self.writer.lock().unwrap();
        writer.append(QueueOp::RemoveQueue(id));
        writer.publish();
    }

    /// Pop from any producer
    pub fn pop_any(&self) -> Option<T> {
        let reader = self.reader.enter().unwrap();
        reader.0.iter().find_map(|(_, q)| SpmcBounded::pop(&*q))
    }

    pub fn pop_from(&self, id: Uuid) -> Result<Option<T>, QueueError> {
        let reader = self.reader.enter().unwrap();
        let q = reader.0.get(&id).ok_or(QueueError::QueueNotFound)?;
        Ok(q.pop())
    }

    pub fn clear(&self, id: Uuid) -> Result<(), QueueError> {
        let reader = self.reader.enter().unwrap();
        let q = reader.0.get(&id).ok_or(QueueError::QueueNotFound)?;
        q.clear();
        Ok(())
    }
}
