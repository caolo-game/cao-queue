mod table;

use std::{
    cell::UnsafeCell, mem::swap, sync::atomic::AtomicIsize, sync::atomic::AtomicU8,
    sync::atomic::Ordering, sync::Arc,
};

use parking_lot::Mutex;
use table::Table;

use crate::MessageId;

const MAX_LOAD_FACTOR: f32 = 0.7;

/// Intenally the map will double-buffer tables to ensure concurrent reads can execute even while
/// the map is resizing. As such it is a bit of a memory-hog.
///
/// TODO: we could count active readers and garbage collect the secondary buffer once no readers
/// remain?
pub struct MessageMap<T> {
    resize_lock: Mutex<()>,
    buffer_ind: AtomicU8,
    count: AtomicIsize,

    // To avoid consistency issues when resizing we'll put values behind Arc's
    // Double buffering to allow reads during resize operations
    kvs: UnsafeCell<[Table<T>; 2]>,
}

unsafe impl<T> Send for MessageMap<T> {}
unsafe impl<T> Sync for MessageMap<T> {}

impl<T> MessageMap<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        let res = Self {
            buffer_ind: AtomicU8::new(0),
            resize_lock: Mutex::new(()),
            kvs: UnsafeCell::new([Table::new(0), Table::new(0)]),
            count: AtomicIsize::new(0),
        };
        res.adjust_size(capacity.max(16));
        res
    }

    pub fn get(&self, id: MessageId) -> Option<Arc<T>> {
        let key = Key::from_u64(id.0);
        let kvs = self.kvs(self.buffer_ind.load(Ordering::Relaxed) as usize);
        let cap = kvs.keys.len();
        // The author assumes that even in the worst case this funcion returns before "the next"
        // resize will invalidate the old buffer
        let mut ind = key.0 as usize % cap;
        loop {
            let k = kvs.keys[ind].load(Ordering::Relaxed);
            if k == key.0 {
                unsafe { return Some(Arc::clone(&(*kvs.values[ind].as_ptr()).1)) }
            } else if k == 0 {
                return None;
            }
            ind = (ind + 1) % cap;
        }
    }

    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed) as usize
    }

    pub fn insert(&self, id: MessageId, value: T) {
        let ind = self.buffer_ind.load(Ordering::Relaxed) as usize;
        let count = self.count.load(Ordering::Relaxed);
        let mut kvs = self.kvs(ind);
        let cap = kvs.keys.len();
        if (count + 1) as f32 > cap as f32 * MAX_LOAD_FACTOR {
            self.grow(cap);
            // reload the index
            let ind = self.buffer_ind.load(Ordering::Acquire);
            kvs = self.kvs(ind as usize);
        }
        let key = Key::from_u64(id.0);
        kvs.insert(key, (id, Arc::new(value)));
        self.count.fetch_add(1, Ordering::Release);
    }

    fn grow(&self, current_cap: usize) {
        let new_cap = current_cap.max(3) * 3 / 2;
        self.adjust_size(new_cap);
    }

    fn kvs(&self, ind: usize) -> &mut Table<T> {
        unsafe { &mut (*self.kvs.get())[ind as usize] }
    }

    fn adjust_size(&self, capacity: usize) {
        let _lock = self.resize_lock.lock();
        // another thread may have already resized so check for that
        let ind = self.buffer_ind.load(Ordering::Acquire) as usize;
        let kvs = self.kvs(ind);
        if kvs.keys.len() >= capacity {
            return;
        }
        // allocate new buffers
        let mut new_kvs = Table::new(capacity);
        // copy current
        unsafe {
            debug_assert!(kvs.keys.len() < new_kvs.keys.len());
            for (key, value) in kvs.keys.iter().zip(kvs.values.iter()) {
                let key = key.load(Ordering::Acquire);
                if key != 0 {
                    let (id, val) = &*value.as_ptr();
                    new_kvs.insert(Key(key), (*id, Arc::clone(val)));
                }
            }
        }

        // swap with the current
        let new_ind = 1 - ind as usize;
        let kvs = self.kvs(new_ind);
        swap(kvs, &mut new_kvs);
        self.buffer_ind.store(new_ind as u8, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Key(u32);

impl Key {
    pub fn from_u32(key: u32) -> Self {
        const MASK: u64 = u32::MAX as u64;

        let mut key = key.max(1) as u64; // ensure non-zero key
        key = (((key >> 16) ^ key) * 0x45d0f3b) & MASK;
        key = (((key >> 16) ^ key) * 0x45d0f3b) & MASK;
        key = ((key >> 16) ^ key) & MASK;
        debug_assert!(key != 0);
        Self(key as u32)
    }

    pub fn from_u64(key: u64) -> Self {
        const MASK: u64 = u32::MAX as u64;

        let mut key = key.max(1); // ensure non-zero key
        key = (((key >> 16) ^ key) * 0x45d0f3b) & MASK;
        key = (((key >> 16) ^ key) * 0x45d0f3b) & MASK;
        key = ((key >> 16) ^ key) & MASK;
        debug_assert!(key != 0);
        Self(key as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_bunch() {
        let map = MessageMap::with_capacity(1);

        for i in 0..256 {
            // trigger a few resizes
            map.insert(MessageId(i), "hello world".to_string());
        }
    }
}
