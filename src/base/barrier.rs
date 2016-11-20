use std::sync::{Mutex, Condvar};
use std::time::Duration;

pub struct Barrier {
    n: u32,
    state: Mutex<State>,
    cvar: Condvar
}

struct State {
    count: u32,
    generation: u32
}

impl Barrier {
    pub fn new(n: u32) -> Barrier {
        Barrier {
            n: n,
            state: Mutex::new(State {
                count: 0,
                generation: 0
            }),
            cvar: Condvar::new()
        }
    }

    pub fn wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.count += 1;

        let generation = state.generation;

        if state.count < self.n {
            while generation == state.generation && state.count < self.n {
                state = self.cvar.wait(state).unwrap();
            }
        } else {
            state.count = 0;
            state.generation += 1;
            self.cvar.notify_all();
        }
    }

    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap();
        state.count += 1;

        let generation = state.generation;

        if state.count < self.n {
            while generation == state.generation && state.count < self.n {
                let result = self.cvar.wait_timeout(state, timeout).unwrap();
                state = result.0;

                if result.1.timed_out() {
                    return false;
                }
            }
        } else {
            state.count = 0;
            state.generation += 1;
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
