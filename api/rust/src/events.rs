use crate::{Browser, Error, ErrorKind, NetworkBeforeRequest, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Stops the listener when dropped. Drop waits for an in-flight callback to finish.
pub struct NetworkRequestListener {
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Browser {
    /// Delivers request notifications on a background thread.
    ///
    /// The callback also receives a terminal error if the subscription fails.
    /// Dropping the returned listener stops delivery.
    pub fn on_network_request<F>(&self, mut callback: F) -> Result<NetworkRequestListener>
    where
        F: FnMut(Result<NetworkBeforeRequest>) + Send + 'static,
    {
        let subscription = self.inner.subscribe_network_before_request()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let worker = thread::Builder::new()
            .name("selenium-network-events".into())
            .spawn(move || {
                while !worker_stopped.load(Ordering::Acquire) {
                    let result = subscription
                        .next(100)
                        .and_then(NetworkBeforeRequest::from_core);
                    if worker_stopped.load(Ordering::Acquire) {
                        break;
                    }
                    match result {
                        Err(error) if error.kind == ErrorKind::Timeout => continue,
                        Err(error) => {
                            callback(Err(error));
                            break;
                        }
                        Ok(event) => callback(Ok(event)),
                    }
                }
            })
            .map_err(|error| {
                Error::new(
                    ErrorKind::Protocol,
                    format!("cannot start network listener: {error}"),
                )
            })?;
        Ok(NetworkRequestListener {
            stopped,
            worker: Some(worker),
        })
    }
}

impl Drop for NetworkRequestListener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            if worker.thread().id() != thread::current().id() && worker.join().is_err() {
                eprintln!("network request listener panicked");
            }
        }
    }
}
