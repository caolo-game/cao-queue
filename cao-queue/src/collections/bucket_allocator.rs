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
    pub fn allocate_aligned(&self, size: usize, alignment: usize) -> *mut u8 {
        Self::allocate_from(self, size, alignment)
    }

    fn allocate_from(mut allocator: &Self, size: usize, alignment: usize) -> *mut u8 {
        assert!(
            (alignment & (alignment - 1)) == 0,
            "alignment must be a power of two"
        );
        loop {
            let buffer = unsafe { &mut *allocator.buffer.get() };
            let len = buffer.len();
            let head = allocator.head.load(Ordering::Acquire);
            let ptr: *mut u8 = unsafe { buffer.as_mut_ptr().add(head) };
            let (diff, ptr) = align(ptr, alignment);
            debug_assert!(head <= len);
            let new_head = head + diff + size;
            if new_head <= len {
                // can allocate in this instance
                let res = allocator
                    .head
                    .compare_and_swap(head, new_head, Ordering::Release);
                assert_eq!(
                    res, head,
                    "Another thread allocated during this call. This is not allowed"
                );
                // decrement bytes out by the number of bytes skipped
                debug_assert!(diff < alignment);
                allocator.bytes_out.fetch_sub(diff, Ordering::Release);
                return ptr as *mut _;
            }

            if size < len * 2 / 3 {
                debug_assert!(head < len);
                let remaining = len - head;
                // some stupid heuristics:
                // assume that future allocations will fail, so don't serve those bytes
                let out = allocator.bytes_out.fetch_sub(remaining, Ordering::Release);

                if out == remaining {
                    // all allocations have been deallocated
                    allocator.reset(len);
                    continue;
                }
            }
            // else allocate using the next instance
            //
            unsafe {
                allocator = match &*allocator.next.get() {
                    Some(ref next) => next,
                    // next.allocate_aligned(size, alignment),
                    None => {
                        // at least as much memory as requested + padding to amortize allocations
                        let next_ptr = allocator.next.get();
                        let next = Self::new(len.max(size) * 3 / 2);
                        std::ptr::write_unaligned(next_ptr, Some(next));
                        (&*next_ptr).as_ref().unwrap()
                    }
                }
            }
        }
    }

    /// # Safety
    ///
    /// 1 producer thread is allowed per allocator
    pub fn allocate(&self, size: usize) -> *mut u8 {
        self.allocate_aligned(size, 1)
    }

    /// # Safety
    ///
    /// Multiple calls with the same `ptr` results in undefined behaviour.
    /// Calling deallocate with a `ptr` not returned by this instance will result in undefined
    /// behaviour!
    pub fn deallocate(&self, ptr: *const u8, size: usize) {
        let buffer = unsafe { &*self.buffer.get() };
        let len = buffer.len();
        let buffer = buffer.as_ptr();
        unsafe {
            if ptr < buffer || buffer.add(len) <= ptr {
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

/// Return the aligned pointer and the diff from the initial one
///
/// This is a tad bit unsafe, however the author assumes that ptrs never going to be around
/// usize::MAX, and alignment will be fairly small.
#[inline]
fn align<T>(ptr: *const T, alignment: usize) -> (usize, *const T) {
    debug_assert_eq!(
        alignment & (alignment - 1),
        0,
        "alignment must be a power of two!"
    );
    let pp = ptr as usize;
    let modulo = pp & (alignment - 1);
    // alignment - modulo if modulo is not 0
    // else the ptr is already aligned
    let diff = (alignment - modulo) * ((modulo != 0) as usize);
    (diff, (pp + diff) as *const _)
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
    fn allocate_align() {
        let allocator = BucketAllocator::new(512);
        let ptr = allocator.allocate_aligned(128, 16);

        assert_eq!(ptr.align_offset(16), 0);
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
