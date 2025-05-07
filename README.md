# take_lock

**TakeLock** is a minimal, thread-safe container for passing ownership of a single `Box<T>` between threads.

It provides an atomic, lock-free way to `put`, `take`, or `swap` a value across threads.
This is implemented with a fairly straight forward use of atomics.

## Example

```rust
use take_lock::TakeLock;
use std::thread;

let lock = std::sync::Arc::new(TakeLock::default());

let sender = {
    let lock = lock.clone();
    thread::spawn(move || {
        lock.put(Some(Box::new(123))).unwrap();
    })
};

let receiver = {
    let lock = lock.clone();
    thread::spawn(move || {
        loop {
            if let Some(val) = lock.take() {
                assert_eq!(*val, 123);
                break;
            }
        }
    })
};

sender.join().unwrap();
receiver.join().unwrap();

