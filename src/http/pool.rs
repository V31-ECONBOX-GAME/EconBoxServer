//! A fixed-size thread pool.
//!
//! One thread per connection is simpler, but an open server should not let a
//! peer decide how many threads exist. The pool caps that; the bounded queue
//! in front of it turns a flood of connections into backpressure on `accept`
//! instead of unbounded memory growth.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// How many connections may wait per worker before `execute` blocks.
const QUEUE_PER_WORKER: usize = 4;

pub struct Pool {
    sender: Option<SyncSender<Job>>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl Pool {
    /// Start `size` worker threads. `size` is raised to at least 1.
    pub fn new(size: usize) -> Pool {
        let size = size.max(1);
        let (sender, receiver) = mpsc::sync_channel::<Job>(size * QUEUE_PER_WORKER);
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = (0..size)
            .map(|index| {
                let receiver = Arc::clone(&receiver);
                thread::Builder::new()
                    .name(format!("econbox-worker-{index}"))
                    .spawn(move || worker(receiver))
                    .expect("failed to spawn worker thread")
            })
            .collect();
        Pool {
            sender: Some(sender),
            workers,
        }
    }

    /// Queue a job, blocking while the queue is full. Returns `false` once the
    /// pool has begun shutting down.
    pub fn execute<F: FnOnce() + Send + 'static>(&self, job: F) -> bool {
        match &self.sender {
            Some(sender) => sender.send(Box::new(job)).is_ok(),
            None => false,
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        // Dropping the sender makes every worker's `recv` return, which ends
        // the loop below; then we wait for them.
        self.sender = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker(receiver: Arc<Mutex<Receiver<Job>>>) {
    loop {
        // The lock is released before the job runs, so workers do not serialise.
        let job = match receiver.lock() {
            Ok(guard) => guard.recv(),
            Err(_) => return,
        };
        match job {
            Ok(job) => {
                // A panic while serving one connection must not retire a worker
                // for the rest of the process's life.
                if panic::catch_unwind(AssertUnwindSafe(job)).is_err() {
                    eprintln!("econbox-server: a request handler panicked; worker recovered");
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn runs_every_job() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let pool = Pool::new(4);
            for _ in 0..64 {
                let counter = Arc::clone(&counter);
                assert!(pool.execute(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }));
            }
        } // dropping the pool joins the workers
        assert_eq!(counter.load(Ordering::SeqCst), 64);
    }

    #[test]
    fn survives_a_panicking_job() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let pool = Pool::new(1);
            pool.execute(|| panic!("boom"));
            thread::sleep(Duration::from_millis(20));
            let counter = Arc::clone(&counter);
            pool.execute(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            });
        }
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
