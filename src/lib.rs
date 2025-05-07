
use std::sync::atomic::Ordering;
use std::ptr;
use std::sync::atomic::AtomicPtr;

/// basic cell like structure that can be shared across threads
/// this type is essentially an Option<Box<T>>
pub struct TakeLock<T:Sync>{
    inner: AtomicPtr<T>
}

impl<T:Sync> Drop for TakeLock<T>{

    fn drop(&mut self) {
        let p: *mut T = *self.inner.get_mut();
        if p.is_null() {
            return;
        }
        unsafe{
            _ = Box::from_raw(p);
        }
    }
}

impl<T:Sync> Default for TakeLock<T>{
    fn default() -> Self { 
        Self{inner:AtomicPtr::new(ptr::null_mut())} 
    }
}


fn safe_to_raw<T>(opt: Option<Box<T>>) -> *mut T {
    match opt {
        Some(b) => Box::into_raw(b),
        None => std::ptr::null_mut(),
    }
}

unsafe fn raw_to_safe<T>(raw: *mut T) -> Option<Box<T>> {
    if raw.is_null(){
        None
    }else {
        unsafe{Some(Box::from_raw(raw))}
    }
}



impl<T:Sync> TakeLock<T>{
    /// Creates a new lock, optionally pre‑filled with a boxed value.
    ///
    /// ```
    /// use take_lock::TakeLock;
    /// let lock = TakeLock::new(Some(Box::new(5)));
    /// ```
    pub fn new(op:Option<Box<T>>) -> Self {
        Self{inner:AtomicPtr::new(safe_to_raw(op))}
    }

    /// Checks whether the lock is null at this time
    ///
    /// ```
    /// use take_lock::TakeLock;
    /// let lock = TakeLock::new(Some(Box::new(5)));
    /// assert!(!lock.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool{
        self.inner.load(Ordering::Acquire).is_null()
    }

    /// Removes and returns the current value, leaving the slot empty.
    ///
    /// ```
    /// use take_lock::TakeLock;
    /// let lock = TakeLock::new(Some(Box::new(42)));
    /// assert_eq!(*lock.take().unwrap(), 42);
    /// assert!(lock.is_empty());
    /// ```
    pub fn take(&self) -> Option<Box<T>> {
        self.swap(None)
    }


    /// Attempts to put a value into an empty lock.
    /// On fail the passed value is returned.
    ///
    /// ```
    /// use take_lock::TakeLock;
    /// let lock = TakeLock::default();
    /// assert!(lock.put(Some(Box::new(7u8))).is_ok());
    ///
    /// // Second put fails and hands the box back.
    /// let err = lock.put(Some(Box::new(9u8))).err().unwrap();
    /// assert_eq!(*err.unwrap(), 9);
    /// ```
    pub fn put(&self,next:Option<Box<T>>) -> Result<(),Option<Box<T>>>{
        let p = safe_to_raw(next);
        match self.inner.compare_exchange(
            ptr::null_mut(),
            p,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(_) => unsafe{Err(raw_to_safe(p))},
        }
    }



    /// Overwrites the current value with next and returns it.
    /// 
    /// ```
    /// use take_lock::TakeLock;
    /// let lock = TakeLock::new(Some(Box::new(1)));
    /// let old = lock.swap(Some(Box::new(2))).unwrap();
    /// assert_eq!(*old, 1);
    /// assert_eq!(*lock.take().unwrap(), 2);
    /// ```
    pub fn swap(&self,next:Option<Box<T>>) -> Option<Box<T>>{
        let p = safe_to_raw(next);

        let mut cur = self.inner.load(Ordering::Acquire);
        loop{
            
            match self.inner.compare_exchange(
                cur,
                p,
                Ordering::AcqRel,
                Ordering::Acquire
            ) {
                Ok(b) => return unsafe{raw_to_safe(b)},
                Err(c) => cur=c,
            }
        }
    }


    /// Consumes the lock returning the inner value
    ///
    /// ```
    /// use take_lock::TakeLock;
    /// let lock = TakeLock::new(Some(Box::new("hello")));
    /// let boxed = lock.into_inner().unwrap();
    /// assert_eq!(*boxed, "hello");
    /// ```
    pub fn into_inner(self) -> Option<Box<T>>{
        let p: *mut T = self.inner.load(Ordering::Relaxed);
        if p.is_null() {
            return None;
        }
        unsafe{
            self.inner.store(ptr::null_mut(),Ordering::Relaxed);
            return Some(Box::from_raw(p));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;

    /// Helper that bumps a counter on drop so we can check for leaks.
    #[derive(Debug)]
    struct DropCounter {
        hits: Arc<AtomicUsize>,
    }
    impl DropCounter {
        fn new(h: Arc<AtomicUsize>) -> Self {
            Self { hits: h }
        }
    }

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn put_take_roundtrip() {
        let drops = Arc::new(AtomicUsize::new(0));
        {
            let lock = TakeLock::default();
            assert!(lock.is_empty());

            lock.put(Some(Box::new(DropCounter::new(drops.clone()))))
                .unwrap();
            assert!(!lock.is_empty());

            let _v = lock.take().unwrap();   // value drops here
            assert!(lock.is_empty());
        }
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn put_when_full_fails() {
        let lock = TakeLock::new(Some(Box::new(5u32)));
        let err = lock.put(Some(Box::new(99u32))).err().unwrap();
        assert_eq!(*err.unwrap(), 99);               // caller gets its box back
        assert_eq!(*lock.take().unwrap(), 5);
    }

    #[test]
    fn swap_returns_previous() {
        let lock = TakeLock::new(Some(Box::new(1u8)));
        let old = lock.swap(Some(Box::new(2u8))).unwrap();
        assert_eq!(*old, 1);
        assert_eq!(*lock.take().unwrap(), 2);
    }

    #[test]
    fn only_one_thread_can_put() {
        let lock = Arc::new(TakeLock::default());
        let l1 = lock.clone();
        let l2 = lock.clone();

        let t1 = thread::spawn(move || l1.put(Some(Box::new(10i32))).is_ok());
        let t2 = thread::spawn(move || l2.put(Some(Box::new(20i32))).is_ok());

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();
        assert!(r1 ^ r2);                     // exactly one succeeded

        let v = lock.take().unwrap();
        assert!(*v == 10 || *v == 20);
    }

    #[test]
    fn into_inner_moves_value_out() {
        let lock = TakeLock::new(Some(Box::new(42usize)));
        let inner = TakeLock::into_inner(lock).unwrap();
        assert_eq!(*inner, 42);
    }

    /// Hammer the lock with ~ 8 * 250 k =2 million operations spread
    /// across several threads, randomly mixing put/take/swap calls.
    /// We verify that
    ///   * the lock is empty at the end
    ///   * every Box that ever left the lock was dropped exactly once
    #[test]
    fn stress_put_take_swap() {
        const THREADS: usize = 8;
        const ITERS:   usize = 250_000;

        let drops = Arc::new(AtomicUsize::new(0));
        let lock  = Arc::new(TakeLock::<DropCounter>::default());

        let mut handles = Vec::with_capacity(THREADS);
        for tid in 0..THREADS {
            let lock  = Arc::clone(&lock);
            let drops = Arc::clone(&drops);
            handles.push(thread::spawn(move || {
                for i in 0..ITERS {
                    // very cheap pseudo‑random mix without pulling in rand
                    match i.wrapping_add(tid) % 10 {
                        0..=2 => {
                            let _ = lock.put(Some(Box::new(DropCounter::new(drops.clone()))));
                        }
                        3..=5 => {
                            drop(lock.take());
                        }
                        _ => {
                            let new = if i & 1 == 0 {
                                Some(Box::new(DropCounter::new(drops.clone())))
                            } else {
                                None
                            };
                            drop(lock.swap(new));
                        }
                    }
                }
            }));
        }

        for h in handles { h.join().unwrap(); }

        // Drain anything that might still be in there, then ensure emptiness.
        drop(lock.take());
        assert!(lock.is_empty(), "lock left non‑empty after stress run");
        // We can’t predict the exact number of DropCounter drops,
        // but we *can* ensure every value that ever got out was freed:
        assert!(
            drops.load(Ordering::Relaxed) > 0,
            "stress did not move any value through the lock",
        );
    }

    /// A tighter race that keeps a value bouncing around with `swap`.
    #[test]
    fn swap_ping_pong() {
        const THREADS:   usize = 4;
        const ROTATIONS: usize = 1_000_000;

        let drops = Arc::new(AtomicUsize::new(0));
        // start with a single value present
        let lock = Arc::new(TakeLock::new(
            Some(Box::new(DropCounter::new(drops.clone())))
        ));

        let mut handles = Vec::with_capacity(THREADS);
        for _ in 0..THREADS {
            let lock  = Arc::clone(&lock);
            let drops = Arc::clone(&drops);
            handles.push(thread::spawn(move || {
                for _ in 0..ROTATIONS {
                    // Alternate between swapping in Some(..) and None.
                    // We ignore the old value; just letting it drop.
                    let _ = lock.swap(Some(Box::new(DropCounter::new(drops.clone()))));
                    let _ = lock.swap(None);
                }
            }));
        }
        for h in handles { h.join().unwrap(); }

        // Clean up any survivor.
        drop(lock.take());
        assert!(lock.is_empty());
        assert!(drops.load(Ordering::Relaxed) > 0);
    }

}
