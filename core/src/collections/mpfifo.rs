use std::{
    cell::UnsafeCell, marker::Unpin, mem::MaybeUninit, pin::Pin, sync::atomic::AtomicUsize,
    sync::atomic::Ordering,
};

use super::QueueError;

type FixMessageBuffer<T> = Pin<Box<[MaybeUninit<T>]>>;

/// Multi-producer FIFO queue
pub struct MpFifo<T: Copy> {
    next: AtomicUsize,
    ready: AtomicUsize,
    head: AtomicUsize,
    tail: AtomicUsize,

    size_mask: usize,
    buffer: UnsafeCell<FixMessageBuffer<T>>,
}

unsafe impl<T: Copy> Sync for MpFifo<T> {}
unsafe impl<T: Copy> Send for MpFifo<T> {}

impl<T> MpFifo<T>
where
    T: Unpin + Copy,
{
    pub fn new(size: usize) -> Result<Self, QueueError> {
        if size & (size - 1) != 0 {
            return Err(QueueError::BadSize(size));
        }
        let mut buffer = Vec::with_capacity(size);
        buffer.resize_with(size, MaybeUninit::uninit);
        Ok(Self {
            buffer: UnsafeCell::new(Pin::new(buffer.into_boxed_slice())),
            size_mask: size - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            next: AtomicUsize::new(0),
            ready: AtomicUsize::new(0),
        })
    }

    pub fn push(&self, value: T) -> Result<(), QueueError> {
        // will retry pushing until either the queue is full or it succeeds
        'retry: loop {
            // begin
            // ensure next is read first
            let next = self.next.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            if incr(next, self.size_mask) == tail {
                return Err(QueueError::Full);
            }
            let n = self
                .next
                .compare_and_swap(next, incr(next, self.size_mask), Ordering::Release);
            if n != next {
                // another thread got this slot
                continue 'retry;
            }
            // write the value
            unsafe {
                let slots = &mut *self.buffer.get();
                slots[next] = MaybeUninit::new(value);
            }
            // finished
            //
            // just incrementing is cheaper than cas and we can calculate its value...
            let r = self.ready.fetch_add(1, Ordering::Release);
            let ready = incr(r, self.size_mask);

            // check for doneness on all threads
            if ready == self.next.load(Ordering::Acquire) {
                // all concurrent writes finished, update head
                self.head.store(ready, Ordering::Relaxed);
                // try to reset the 'ready' counter to this actual position
                // but it should be fine if this fails
                self.ready.compare_and_swap(r, ready, Ordering::Relaxed);
            }

            return Ok(());
        }
    }

    /// Pop for when only a single consumer exists
    pub fn pop_single(&self) -> Option<T> {
        let head = self.next.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // copy the value before incrementing tail, as then another thread may write to that
        // position
        let value = unsafe {
            let slots = &*self.buffer.get();
            slots[tail].assume_init()
        };
        self.tail
            .store(incr(tail, self.size_mask), Ordering::Release);
        Some(value)
    }

    /// Pop for when multiple consumers exist
    pub fn pop_multi(&self) -> Option<T> {
        loop {
            let tail = self.tail.load(Ordering::Relaxed);
            let head = self.head.load(Ordering::Acquire);
            if tail == head {
                // empty
                return None;
            }
            let new_tail = incr(tail, self.size_mask);

            let item = unsafe {
                let slots = &*self.buffer.get();
                slots[tail].assume_init()
            };

            let res = self
                .tail
                .compare_and_swap(tail, new_tail, Ordering::Release);
            if res == tail {
                return Some(item);
            }
            // else another thread stole this item, try again
        }
    }
}

#[inline]
fn incr(val: usize, mask: usize) -> usize {
    (val + 1) & mask
}

#[cfg(test)]
mod tests {
    use crate::MessageId;

    use super::*;
    use std::{sync::Arc, sync::Barrier, time::Duration};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn basic_push_pop() {
        let queue: MpFifo<MessageId> = MpFifo::new(16).unwrap(); // lot smaller capacity than items we push
        let queue = Arc::new(queue);

        let bar = Arc::new(Barrier::new(4));
        let producers = {
            let mut producers = Vec::with_capacity(4);
            for i in 0..4 {
                producers.push({
                    let queue = Arc::clone(&queue);
                    let bar = Arc::clone(&bar);
                    std::thread::spawn(move || {
                        bar.wait(); // sync threads here to simulate contented queues
                        for j in 0..128 {
                            'retry: loop {
                                match queue.push(MessageId(i * 128 + j)) {
                                    Ok(_) => break 'retry,
                                    Err(QueueError::Full) => continue 'retry,
                                    e @ _ => panic!("push failed {:?}", e),
                                }
                            }
                        }
                    })
                })
            }
            producers
        };

        let mut seen = vec![0; 128 * 4];
        let mut it = 0;
        while seen.iter().any(|x| *x == 0) && it < 1024 {
            it += 1;
            let msg = queue.pop_multi();
            if let Some(msg) = msg {
                assert!(msg.0 < 128 * 4);
                seen[msg.0 as usize] += 1;
            } else {
                std::thread::sleep(Duration::from_millis(1))
            }
        }
        assert!(it < 1024, "Timeout");
        assert!(seen.iter().all(|x| *x == 1), "{:?}", seen);

        for producer in producers {
            producer.join().unwrap();
        }
    }

    #[test]
    fn test_full_push_fails() {
        let mut msgs = Vec::with_capacity(17);
        let queue = MpFifo::new(16).unwrap();

        for i in 0..15 {
            // size - 1
            msgs.push(MessageId(i));
            let msg: &MessageId = msgs.last().unwrap();
            queue.push(*msg).unwrap();
        }

        msgs.push(MessageId(69));
        let msg: &MessageId = msgs.last().unwrap();

        // last push should err
        queue.push(*msg).unwrap_err();

        queue.pop_single().unwrap();

        queue.push(*msg).unwrap();
    }
}
