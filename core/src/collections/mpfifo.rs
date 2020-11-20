use std::{
    cell::UnsafeCell, convert::TryFrom, marker::Unpin, mem::MaybeUninit, pin::Pin,
    sync::atomic::spin_loop_hint, sync::atomic::AtomicU64, sync::atomic::Ordering,
};

use super::QueueError;

type FixMessageBuffer<T> = Pin<Box<[MaybeUninit<T>]>>;

/// Multi-producer FIFO queue
///
// implementation note: timestamps of `_ts` mean that that data is monotonically increasing
pub struct MpFifo<T: Copy> {
    next: AtomicU64,
    ready_ts: AtomicU64,
    head: AtomicU64,
    tail: AtomicU64,

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
            size_mask: size - 1,
            buffer: UnsafeCell::new(Pin::new(buffer.into_boxed_slice())),
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            next: AtomicU64::new(0),
            ready_ts: AtomicU64::new(0),
        })
    }

    pub fn push(&self, value: T) -> Result<(), QueueError> {
        // will retry pushing until either the queue is full or it succeeds
        'retry: loop {
            // begin
            // ensure next is read first
            let next = self.next.load(Ordering::Acquire);
            let new_next = incr(next, self.size_mask);
            let tail = self.tail.load(Ordering::Acquire);
            if new_next as u64 == tail {
                return Err(QueueError::Full);
            }
            let n = self
                .next
                .compare_and_swap(next, new_next as u64, Ordering::Release);
            if n != next {
                // another thread got this slot
                spin_loop_hint(); // signal the cpu to enter low-power mode
                continue 'retry;
            }
            // write the value
            unsafe {
                let slots = &mut *self.buffer.get();
                slots[next as usize] = MaybeUninit::new(value);
            }
            // finished
            //
            // just incrementing is cheaper than cas and we can calculate its value...
            let r = self.ready_ts.fetch_add(1, Ordering::AcqRel);
            let ready = incr(r, self.size_mask) as u64;

            // check for doneness on all threads
            if ready == self.next.load(Ordering::Acquire) {
                // all concurrent writes finished, update head
                self.head.store(ready, Ordering::Release);
            }

            return Ok(());
        }
    }

    /// Pop for when only a single consumer exists
    pub fn pop_single(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // copy the value before incrementing tail, as then another thread may write to that
        // position
        let value = unsafe {
            let slots = &*self.buffer.get();
            slots[tail as usize].assume_init()
        };
        self.tail
            .store(incr(tail, self.size_mask) as u64, Ordering::Release);
        Some(value)
    }

    /// Pop for when multiple consumers exist
    pub fn pop_multi(&self) -> Option<T> {
        todo!()
    }
}

#[inline]
fn incr(val: u64, mask: usize) -> usize {
    (usize::try_from(val).unwrap() + 1) & mask
}

#[cfg(test)]
mod tests {
    use crate::MessageId;

    use super::*;
    use std::{sync::Arc, sync::Barrier, time::Duration, time::Instant};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn basic_push_pop() {
        const PRODUCERS: u64 = 8;

        let queue: MpFifo<MessageId> = MpFifo::new(16).unwrap();
        let queue = Arc::new(queue);

        let bar = Arc::new(Barrier::new(PRODUCERS as usize + 1));
        let producers = {
            let mut producers = Vec::with_capacity(PRODUCERS as usize);
            for i in 0..PRODUCERS {
                producers.push({
                    let queue = Arc::clone(&queue);
                    let bar = Arc::clone(&bar);
                    std::thread::spawn(move || {
                        bar.wait(); // sync threads here to simulate contented queues
                        for j in 0..100 {
                            'retry: loop {
                                match queue.push(MessageId(i * 100 + j)) {
                                    Ok(_) => break 'retry,
                                    Err(QueueError::Full) => continue 'retry,
                                    e @ _ => panic!("push failed {:?}", e),
                                }
                            }
                        }
                        eprintln!("thread {} exit", i);
                    })
                })
            }
            producers
        };

        let mut seen = vec![0; 100 * PRODUCERS as usize];
        let mut it = 0u64;
        let mut pops = 0;
        bar.wait();
        let start = Instant::now();
        while pops < 100 * PRODUCERS && (Instant::now() - start) < Duration::from_secs(3) {
            // pretty big leeway....
            it += 1;
            let msg = queue.pop_single();
            if let Some(msg) = msg {
                assert!(msg.0 < 100 * PRODUCERS);
                seen[msg.0 as usize] += 1;
                pops += 1;
            }
        }
        println!("Pops: {} It: {}", pops, it);
        let invalid = seen
            .iter()
            .enumerate()
            .filter(|(_, x)| **x != 1)
            .collect::<Vec<_>>();
        let seen_all = invalid.is_empty();
        assert!(seen_all, "Invalid items: {}\n{:?}", invalid.len(), invalid);
        for producer in producers {
            producer.thread();
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
