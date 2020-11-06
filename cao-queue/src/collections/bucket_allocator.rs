//! Multi-threaded allocator. Assuming a single 'producer' thread and multiple 'consumer' threads.
//!
//! Only a single thread may call 'allocate' at a time, however any number of threads may call
//! 'deallocate' at the same time.
//!
use std::{cell::UnsafeCell, pin::Pin, sync::atomic::AtomicUsize, sync::atomic::Ordering};

pub struct BucketAllocator {
    buffer: UnsafeCell<Pin<Box<[u8]>>>,
    head: AtomicUsize,
    bytes_out: AtomicUsize,

    next: UnsafeCell<Option<Pin<Box<Self>>>>,
}

impl BucketAllocator {
    pub fn new(capacity: usize) -> Pin<Box<Self>> {
        Box::pin(Self {
            buffer: UnsafeCell::new(Pin::new(vec![0u8; capacity].into_boxed_slice())),
            head: AtomicUsize::new(0),
            bytes_out: AtomicUsize::new(capacity),

            next: UnsafeCell::new(None),
        })
    }

    /// Total capacity of this allocator chain
    pub fn capacity(&self) -> usize {
        unsafe {
            let next = &*self.next.get();
            (*self.buffer.get()).len() + next.as_ref().map(|alloc| alloc.capacity()).unwrap_or(0)
        }
    }

    /// # Safety
    ///
    /// 1 producer thread is allowed per allocator
    pub fn allocate(&self, size: usize) -> *mut u8 {
        let buffer = unsafe { &mut *self.buffer.get() };
        let len = buffer.len();
        let head = self.head.load(Ordering::Acquire);
        if head + size <= len {
            // can allocate in this instance
            let res = self
                .head
                .compare_and_swap(head, head + size, Ordering::Release);
            assert_eq!(
                res, head,
                "Another thread allocated during this call. This is not allowed"
            );
            return unsafe { buffer.as_mut_ptr().add(res) };
        }
        if size < len * 2 / 3 {
            debug_assert!(head < len);
            let remaining = len - head;
            // some stupid heuristics:
            // assume that future allocations will fail, so don't serve those bytes
            let out = self.bytes_out.fetch_sub(remaining, Ordering::Release);

            if out == remaining {
                // all allocations have been deallocated
                self.reset(len);
                return self.allocate(size);
            }
        }
        // else allocate using the next instance
        //
        unsafe {
            match &*self.next.get() {
                Some(ref next) => next.allocate(size),
                None => {
                    let next = Self::new(len.max(size));
                    let res = next.allocate(size);
                    std::ptr::write_unaligned(self.next.get(), Some(next));
                    res
                }
            }
        }
    }

    /// # Safety
    ///
    /// Multiple calls with the same `ptr` results in undefined behaviour
    pub fn deallocate(&self, ptr: *const u8, size: usize) {
        let buffer = unsafe { &*self.buffer.get() };
        let len = buffer.len();
        let b = buffer.as_ptr();
        unsafe {
            if ptr < b || b.add(len) <= ptr {
                // `ptr` was not allocated by this instance
                let next = &mut *self.next.get();
                return next.as_mut().expect("`ptr` was not allocated by this instance. Expected a `next` instance to be present").deallocate(ptr, size);
            }
            // else the ptr belongs to this instance
            let out = self.bytes_out.fetch_sub(size, Ordering::Release);
            if out == size {
                self.reset(len);
            }
        }
    }

    fn reset(&self, len: usize) {
        self.bytes_out.store(len, Ordering::Release);
        self.head.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_allocate() {
        let allocator = BucketAllocator::new(128);
        let bytes = allocator.allocate(128);
        let res = b"pogchamp";
        unsafe {
            let ptr = bytes as *mut _;
            std::ptr::write_unaligned(ptr, *res);

            assert_eq!(*res, *ptr);
        }
    }

    #[test]
    fn can_reuse_bucket() {
        let allocator = BucketAllocator::new(128);
        let ptr = allocator.allocate(128);
        allocator.deallocate(ptr, 128);
        let _ptr = allocator.allocate(128);

        // assert that no new instance has been created
        unsafe { assert!((*allocator.next.get()).is_none()) }
    }

    #[test]
    fn can_expand() {
        let allocator = BucketAllocator::new(16);

        let ptr = allocator.allocate(128);
        let res = b"pogchamp";
        unsafe {
            let ptr = ptr as *mut _;
            std::ptr::write_unaligned(ptr, *res);

            assert_eq!(*res, *ptr);
        }

        allocator.deallocate(ptr, 128);

        unsafe {
            assert!((*allocator.next.get()).is_some());
        }

        assert!(allocator.capacity() >= 128 + 16);
    }
}
