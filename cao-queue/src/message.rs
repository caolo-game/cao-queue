use std::{io::Write, ptr::NonNull, slice};

use crate::{collections::bucket_allocator::BucketAllocator, MessageId};

pub struct ManagedMessage<'a> {
    pub id: MessageId,
    data: NonNull<u8>,
    len: usize,
    capacity: usize,
    allocator: &'a BucketAllocator,
}

impl<'a> Write for ManagedMessage<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // at most the remaining capacity
        let to_write = buf.len().min(self.capacity - self.len);
        if to_write > 0 {
            unsafe {
                let s = slice::from_raw_parts_mut(self.data.as_ptr().add(self.len), to_write);
                s.copy_from_slice(buf);
            }
            self.len += to_write;
            debug_assert!(self.len <= self.capacity);
        }
        Ok(to_write)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> Drop for ManagedMessage<'a> {
    fn drop(&mut self) {
        let ptr = self.data.as_ptr();
        self.allocator.deallocate(ptr as *mut u8, self.capacity);
    }
}

impl<'a> ManagedMessage<'a> {
    pub fn new(id: MessageId, capacity: usize, allocator: &'a BucketAllocator) -> Self {
        let ptr = allocator.allocate(capacity).cast();
        unsafe {
            Self {
                id,
                data: NonNull::new_unchecked(ptr),
                len: 0,
                capacity,
                allocator,
            }
        }
    }

    pub fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.data.as_ptr(), self.len) }
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.data.as_ptr(), self.len) }
    }
}
