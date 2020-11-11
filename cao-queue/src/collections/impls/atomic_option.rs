use core::{
    intrinsics::transmute, mem::replace, mem::MaybeUninit, sync::atomic::AtomicU8,
    sync::atomic::Ordering,
};

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[repr(u8)]
enum State {
    /// No item is set
    Empty,
    /// Item is being set
    Contested,
    /// Item is set
    Filled,
}

/// Atomically set optional...
pub struct AtomicOption<T> {
    state: AtomicU8,
    item: MaybeUninit<T>,
}

impl<T> Drop for AtomicOption<T> {
    fn drop(&mut self) {
        if self.state.load(Ordering::SeqCst) == State::Filled as u8 {
            // drop the current item
            let _item = unsafe { replace(&mut self.item, MaybeUninit::uninit()).assume_init() };
        }
    }
}

impl<T> Default for AtomicOption<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AtomicOption<T> {
    pub const fn new() -> Self {
        Self {
            state: AtomicU8::new(State::Empty as u8),
            item: MaybeUninit::uninit(),
        }
    }

    pub fn new_fill(value: T) -> Self {
        Self {
            state: AtomicU8::new(State::Filled as u8),
            item: MaybeUninit::new(value),
        }
    }

    pub fn is_some(&self) -> bool {
        self.state.load(Ordering::Acquire) == State::Filled as u8
    }

    pub fn is_none(&self) -> bool {
        !self.is_some()
    }

    pub fn as_ref(&self) -> Option<&T> {
        let state = unsafe { transmute(self.state.load(Ordering::SeqCst)) };
        match state {
            State::Empty => None,
            State::Contested => None,
            State::Filled => unsafe { Some(&*self.item.as_ptr()) },
        }
    }

    #[allow(unused)]
    pub fn as_mut(&mut self) -> Option<&mut T> {
        let state = unsafe { transmute(self.state.load(Ordering::SeqCst)) };
        match state {
            State::Empty => None,
            State::Contested => None,
            State::Filled => unsafe { Some(&mut *self.item.as_mut_ptr()) },
        }
    }

    pub fn as_ref_or_insert_with<F>(&self, f: F) -> &T
    where
        F: FnOnce() -> T,
    {
        let state = self.state.compare_and_swap(
            State::Empty as u8,
            State::Contested as u8,
            Ordering::SeqCst,
        );
        unsafe {
            match transmute(state) {
                State::Empty => {
                    // let's go
                    let value = f();
                    let state = self.write(value);
                    assert!(state == State::Contested);
                }
                State::Contested => self.wait_for_fill(),
                State::Filled => {}
            }
        }
        self.as_ref().unwrap()
    }

    /// returns the last state
    unsafe fn write(&self, value: T) -> State {
        core::ptr::write(self.item.as_ptr() as *mut _, value);
        let state = self.state.compare_and_swap(
            State::Contested as u8,
            State::Filled as u8,
            Ordering::SeqCst,
        );
        transmute(state)
    }

    fn wait_for_fill(&self) {
        // TODO: timeout?
        // altough I don't expect to be able to handle another thread crashing at this
        // point
        let mut state;
        loop {
            state = self.state.load(Ordering::Acquire);
            if state != State::Contested as u8 {
                break;
            }
        }
        debug_assert_eq!(state, State::Filled as u8);
    }

    /// Returns if the `set` was successful
    /// If it's false then another thread set the value before this could.
    ///
    /// However after calling `set` the value will always be filled.
    pub fn set(&self, value: T) -> bool {
        let state = self.state.compare_and_swap(
            State::Empty as u8,
            State::Contested as u8,
            Ordering::SeqCst,
        );
        unsafe {
            match transmute(state) {
                State::Empty => {
                    // let's go
                    let state = self.write(value);
                    debug_assert!(state == State::Contested);
                    true
                }
                State::Contested => {
                    // another thread is currently setting the value
                    // wait for it to succeed then return false
                    self.wait_for_fill();
                    false
                }
                // this link is already filled
                State::Filled => false,
            }
        }
    }
}
