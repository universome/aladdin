use std::iter::Iterator;
use std::time::{Duration, Instant};
use std::thread::sleep;

pub struct Periodic {
    interval: u64,
    timestamp: Instant
}

impl Periodic {
    pub fn new(interval: u32) -> Periodic {
        Periodic {
            interval: interval as u64,
            timestamp: Instant::now() - Duration::new(interval as u64, 0)
        }
    }

    pub fn next_if_elapsed(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.timestamp);

        if elapsed.as_secs() < self.interval {
            return false;
        }

        self.timestamp = now;

        true
    }
}

impl Iterator for Periodic {
    type Item = ();

    fn next(&mut self) -> Option<()> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.timestamp);

        if elapsed.as_secs() < self.interval {
            sleep(Duration::new(self.interval, 0) - elapsed);
            self.timestamp = Instant::now();
        } else {
            self.timestamp = now;
        }

        Some(())
    }
}
