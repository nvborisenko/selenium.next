use selenium_core::{Error, Result};
use std::future::Future;
use std::sync::{mpsc, LazyLock};
use tokio::runtime::Runtime;

static RUNTIME: LazyLock<std::io::Result<Runtime>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("selenium-sync")
        .build()
});

pub(crate) fn run<F, T>(future: F) -> Result<T>
where
    F: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let runtime = RUNTIME.as_ref().map_err(|error| {
        Error::protocol(format!("cannot start synchronous adapter runtime: {error}"))
    })?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let task = runtime.spawn(future);
    runtime.spawn(async move {
        let result = task
            .await
            .map_err(|error| Error::protocol(format!("synchronous adapter task failed: {error}")))
            .and_then(|result| result);
        let _ = sender.send(result);
    });
    receiver.recv().map_err(|error| {
        Error::protocol(format!(
            "synchronous adapter task did not complete: {error}"
        ))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synchronous_caller_receives_result_and_error() {
        assert_eq!(run(async { Ok(42) }).unwrap(), 42);
        let error = run::<_, ()>(async { Err(Error::protocol("failed")) }).unwrap_err();
        assert_eq!(error.message, "failed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn synchronous_caller_works_inside_an_existing_runtime() {
        assert_eq!(run(async { Ok(42) }).unwrap(), 42);
    }

    #[test]
    fn task_panic_is_reported_as_error() {
        let error = run::<_, ()>(async { panic!("task failed") }).unwrap_err();
        assert!(error.message.contains("synchronous adapter task failed"));
    }
}
