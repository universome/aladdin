use std::time::Duration;
use parking_lot::{Mutex, Condvar};

pub struct Barrier {
    n: u32,
    count: Mutex<u32>,
    cvar: Condvar
}

impl Barrier {
    pub fn new(n: u32) -> Barrier {
        Barrier {
            n: n,
            count: Mutex::new(0),
            cvar: Condvar::new()
        }
    }

    pub fn wait(&self) {
        let mut count = self.count.lock();
        *count += 1;

        if *count < self.n {
            self.cvar.wait(&mut count);
        } else {
            *count = 0;
            self.cvar.notify_all();
        }
    }

    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let mut count = self.count.lock();
        *count += 1;

        if *count < self.n {
            !self.cvar.wait_for(&mut count, timeout).timed_out()
        } else {
            *count = 0;
            self.cvar.notify_all();
            true
        }
    }
}

#[test]
fn test_barrier() {
    use std::sync::Arc;
    use std::sync::mpsc::{channel, TryRecvError};
    use std::thread;

    const N: usize = 10;

    let barrier = Arc::new(Barrier::new(N as u32));
    let (tx, rx) = channel();

    for _ in 0..N-1 {
        let barrier = barrier.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            barrier.wait();
            tx.send(1).unwrap();
        });
    }

    assert!(match rx.try_recv() {
        Err(TryRecvError::Empty) => true,
        _ => false,
    });

    barrier.wait();
    let sum = 1 + rx.into_iter().take(N-1).sum::<usize>();

    assert_eq!(sum, N);
}

#[test]
fn test_barrier_timeout() {
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    const N: u32 = 5;

    let barrier = Arc::new(Barrier::new(N + 1));

    for i in 1..N+1 {
        let barrier = barrier.clone();

        thread::spawn(move || {
            let start = Instant::now();
            let timeout = Duration::new(0, i as u32 * 1000000);

            let is_ok = barrier.wait_timeout(timeout);

            assert!(!is_ok);
            assert!(start.elapsed() >= timeout);
        });
    }
}
