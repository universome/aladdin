use std::sync::{Mutex, Condvar};
use std::time::Duration;

pub struct Barrier {
    count: Mutex<usize>,
    cvar: Condvar
}

impl Barrier {
    pub fn new(count: usize) -> Barrier {
        Barrier {
            count: Mutex::new(count),
            cvar: Condvar::new()
        }
    }

    pub fn wait(&self, timeout: Duration) -> bool {
        let mut count = self.count.lock().unwrap();
        *count -= 1;

        if *count > 0 {
            while *count > 0 {
                let result = self.cvar.wait_timeout(count, timeout).unwrap();
                count = result.0;

                if result.1.timed_out() {
                    return false;
                }
            }
        } else {
            self.cvar.notify_all();
        }

        true
    }
}

#[test]
fn test_barrier() {
    use std::sync::Arc;
    use std::sync::mpsc::{channel, TryRecvError};
    use std::thread;

    const N: usize = 10;

    let long = Duration::new(42, 0);
    let barrier = Arc::new(Barrier::new(N));
    let (tx, rx) = channel();

    for _ in 0..N-1 {
        let barrier = barrier.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            barrier.wait(long);
            tx.send(1).unwrap();
        });
    }

    assert!(match rx.try_recv() {
        Err(TryRecvError::Empty) => true,
        _ => false,
    });

    barrier.wait(long);
    let sum = 1 + rx.into_iter().take(N-1).sum::<usize>();

    assert_eq!(sum, N);
}

#[test]
fn test_barrier_timeout() {
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    const N: usize = 5;

    let barrier = Arc::new(Barrier::new(N + 1));

    for i in 1..N+1 {
        let barrier = barrier.clone();

        thread::spawn(move || {
            let start = Instant::now();
            let timeout = Duration::new(0, i as u32 * 1000000);

            let is_ok = barrier.wait(timeout);

            assert!(!is_ok);
            assert!(start.elapsed() < timeout);
        });
    }
}
