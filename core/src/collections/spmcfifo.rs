//! # SpmcFifo
//!
//! Single Producer Multi Consumer First In First Out Queue
//!
use super::QueueError;

use std::{cell::UnsafeCell, marker::Unpin, pin::Pin, sync::atomic::AtomicUsize};
use std::{mem::MaybeUninit, sync::atomic::Ordering};

type FixMessageBuffer<T> = Pin<Box<[MaybeUninit<T>]>>;

pub struct SpmcFifo<T: Copy> {
    size_mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
    buffer: UnsafeCell<FixMessageBuffer<T>>,
}

unsafe impl<T: Copy> Sync for SpmcFifo<T> {}
unsafe impl<T: Copy> Send for SpmcFifo<T> {}

impl<T> std::fmt::Debug for SpmcFifo<T>
where
    T: std::fmt::Debug + Copy + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpmcFifo")
            .field("head", &self.head.load(Ordering::Relaxed))
            .field("tail", &self.tail.load(Ordering::Relaxed))
            .field("capacity", &(self.size_mask + 1))
            .finish()?;

        write!(f, " Items: ")?;
        let mut debugger = f.debug_list();

        for msg in self.iter() {
            debugger.entry(&msg);
        }
        debugger.finish()
    }
}

struct QIterator<'a, T> {
    buffer: *const MaybeUninit<T>,
    size_mask: usize,
    head: usize,
    tail: usize,
    _m: std::marker::PhantomData<&'a i32>,
}

impl<'a, T: 'a + Copy> Iterator for QIterator<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.head == self.tail {
            return None;
        }
        let item = unsafe { &*self.buffer.add(self.tail) };
        let item = unsafe { *item.as_ptr() };
        self.tail = incr(self.tail, self.size_mask);

        Some(item)
    }
}
impl<T: Copy + 'static> SpmcFifo<T> {
    /// Iterates over the messages without consuming them!
    ///
    /// # Safety
    ///
    /// Iterating while also mutating the queue _may_ result in bad things happening.
    /// Use with care!
    pub fn iter(&self) -> impl Iterator<Item = T> {
        QIterator {
            buffer: unsafe {
                let buf = self.buffer.get();
                let buf = &*buf;
                buf.as_ptr()
            },
            size_mask: self.size_mask,
            head: self.head.load(Ordering::Relaxed),
            tail: self.tail.load(Ordering::Relaxed),
            _m: Default::default(),
        }
    }
}

impl<T> SpmcFifo<T>
where
    T: Unpin + Copy,
{
    /// Size must be a power of two
    ///
    /// Note on size: the queue will actually only hold size -1 items at most
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
        })
    }

    /// # Safety
    ///
    /// Highly unsafe;
    /// Use internally, make sure invariants hold.
    #[allow(clippy::mut_from_ref)]
    unsafe fn buffer_mut(&self) -> &mut [MaybeUninit<T>] {
        let buffer = self.buffer.get();
        let mut_buffer: &mut Pin<Box<[MaybeUninit<T>]>> = &mut *buffer;
        &mut *mut_buffer
    }

    /// Push an item into the queue.
    /// Only 1 producer thread is allowed at a time!
    pub fn push(&self, msg: T) -> Result<(), QueueError> {
        let head = self.head.load(Ordering::Acquire);
        let new_ind = incr(head, self.size_mask);
        if new_ind == self.tail.load(Ordering::Acquire) {
            return Err(QueueError::Full);
        }
        unsafe {
            self.buffer_mut()[head] = MaybeUninit::new(msg);
        }
        let _res = self.head.compare_and_swap(head, new_ind, Ordering::Release);
        debug_assert!(
            _res == head,
            "Contract violation: Another thread pushed while this push was in progress!"
        );
        Ok(())
    }

    /// Pop the last item from queue, if any
    pub fn pop(&self) -> Option<T> {
        loop {
            let tail = self.tail.load(Ordering::Acquire);
            let head = self.head.load(Ordering::Acquire);
            if tail == head {
                // empty
                return None;
            }
            let new_tail = incr(tail, self.size_mask);

            let item = unsafe {
                let item = self.buffer_mut()[tail];
                item.assume_init()
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

    /// To ensure thread safety this method is really slow. Use sparingly
    pub fn clear(&self) {
        while self.pop().is_some() {}
    }

    pub fn is_empty(&self) -> bool {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail == head
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
    use std::{sync::Arc, time::Duration};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn basic_push_pop() {
        let mut msgs = Vec::with_capacity(128);
        let queue: SpmcFifo<MessageId> = SpmcFifo::new(512).unwrap();
        let queue = Arc::new(queue);

        let worker = {
            let queue = Arc::clone(&queue);
            std::thread::spawn(move || {
                let mut seen = vec![0; 128];
                let mut it = 0;
                while seen.iter().any(|x| *x == 0) && it < 512 {
                    it += 1;
                    let msg = queue.pop();
                    if let Some(msg) = msg {
                        assert!(msg.0 < 128);
                        seen[msg.0 as usize] += 1;
                    } else {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                }
                assert!(it < 512, "Timeout");
                seen
            })
        };

        for i in 0..128 {
            msgs.push(MessageId(i));
            let msg: &MessageId = msgs.last().unwrap();
            queue.push(*msg).unwrap();
        }

        let seen = worker.join().unwrap();

        assert_eq!(seen.len(), 128);
        assert!(seen.iter().all(|x| *x == 1), "{:?}", seen);
    }

    #[test]
    fn test_full_push_fails() {
        let mut msgs = Vec::with_capacity(17);
        let queue = SpmcFifo::new(16).unwrap();

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

        queue.pop().unwrap();

        queue.push(*msg).unwrap();
    }
}
