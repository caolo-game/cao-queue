use std::{
    mem::replace, mem::MaybeUninit, sync::atomic::AtomicU32, sync::atomic::Ordering, sync::Arc,
};

use crate::MessageId;

use super::Key;

pub(crate) struct Table<T> {
    pub keys: Box<[AtomicU32]>,
    pub values: Box<[MaybeUninit<(MessageId, Arc<T>)>]>,
}

impl<T> Table<T> {
    pub fn new(cap: usize) -> Self {
        let mut keys = Vec::with_capacity(cap);
        keys.resize_with(cap, || AtomicU32::new(0));

        let mut values = Vec::with_capacity(cap);
        values.resize_with(cap, MaybeUninit::uninit);
        Self {
            keys: keys.into_boxed_slice(),
            values: values.into_boxed_slice(),
        }
    }

    /// TODO: return the old value if any??
    /// Return the index.
    /// Assumes that keys has capacity for the item.
    /// Overwrites the old value if any
    pub fn insert(&mut self, key: Key, val: (MessageId, Arc<T>)) -> usize {
        let cap = self.keys.len();

        let mut i = key.0 as usize % cap;
        for _ in 0..=cap {
            let k = self.keys[i].compare_and_swap(0, key.0, Ordering::AcqRel);
            if k == 0 {
                self.values[i] = MaybeUninit::new(val);
                return i;
            } else if k == key.0 {
                let old = replace(&mut self.values[i], MaybeUninit::new(val));
                unsafe {
                    let _old = old.assume_init();
                }
                return i;
            }
            i = (i + 1) % cap;
        }
        unreachable!(
            "insert assumed that you made sure the Table instance had capacity for the item"
        );
    }
}

impl<T> Drop for Table<T> {
    fn drop(&mut self) {
        for (key, value) in self.keys.iter().zip(self.values.iter_mut()) {
            if key.load(Ordering::Relaxed) != 0 {
                unsafe {
                    let _val: (MessageId, Arc<T>) =
                        replace(value, MaybeUninit::uninit()).assume_init();
                }
            }
        }
    }
}
