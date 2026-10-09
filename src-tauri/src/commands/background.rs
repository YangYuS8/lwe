/// Run synchronous service work without blocking Tauri's command executor.
pub(crate) async fn run_blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        // A JoinError may carry a panic payload. Keep it out of the IPC response.
        .map_err(|_| "Background operation failed".to_string())?
}

/// Desktop snapshots and actions must observe the completed startup restore.
pub(crate) async fn run_desktop_blocking<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    run_blocking(move || {
        crate::services::startup_restore::wait()?;
        task()
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::run_blocking;
    use std::future::Future;
    use std::pin::pin;
    use std::sync::mpsc;
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    #[test]
    fn waiting_background_work_yields_the_command_executor() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut task = pin!(run_blocking(move || {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(())
        }));
        let mut context = Context::from_waker(Waker::noop());

        // A synchronous implementation would wait here until the closure's timeout.
        let first_poll = task.as_mut().poll(&mut context);
        release_tx.send(()).unwrap();
        assert!(matches!(first_poll, Poll::Pending));
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        tauri::async_runtime::block_on(task).unwrap();
    }

    #[test]
    fn background_task_runs_on_a_separate_thread() {
        let caller = std::thread::current().id();
        let worker =
            tauri::async_runtime::block_on(run_blocking(|| Ok(std::thread::current().id())))
                .unwrap();

        assert_ne!(caller, worker);
    }

    #[test]
    fn background_task_preserves_service_failure() {
        let error = tauri::async_runtime::block_on(run_blocking(|| {
            Err::<(), _>("Steam Web API key is required".to_string())
        }))
        .unwrap_err();

        assert_eq!(error, "Steam Web API key is required");
    }

    #[test]
    fn background_task_panic_returns_a_generic_error() {
        let error = tauri::async_runtime::block_on(run_blocking(|| -> Result<(), String> {
            panic!("test background task panic");
        }))
        .unwrap_err();

        assert_eq!(error, "Background operation failed");
        assert!(!error.contains("test background task panic"));
    }
}
