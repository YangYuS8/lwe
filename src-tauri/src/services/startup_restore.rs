use std::sync::{Condvar, Mutex};

#[derive(Default)]
struct RestoreGate {
    pending: Mutex<bool>,
    finished: Condvar,
}

static STARTUP_RESTORE: RestoreGate = RestoreGate {
    pending: Mutex::new(false),
    finished: Condvar::new(),
};

pub(crate) struct RestoreCompletion<'a>(&'a RestoreGate);

impl RestoreGate {
    fn begin(&self) -> Result<RestoreCompletion<'_>, String> {
        *self
            .pending
            .lock()
            .map_err(|_| "Startup restore state was poisoned")? = true;
        Ok(RestoreCompletion(self))
    }

    fn wait(&self) -> Result<(), String> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| "Startup restore state was poisoned")?;
        let _guard = self
            .finished
            .wait_while(pending, |pending| *pending)
            .map_err(|_| "Startup restore state was poisoned")?;
        Ok(())
    }
}

impl Drop for RestoreCompletion<'_> {
    fn drop(&mut self) {
        let mut pending = self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *pending = false;
        self.0.finished.notify_all();
    }
}

pub(crate) fn begin() -> Result<RestoreCompletion<'static>, String> {
    STARTUP_RESTORE.begin()
}

pub(crate) fn wait() -> Result<(), String> {
    STARTUP_RESTORE.wait()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn snapshot_waits_until_background_restore_has_completed() {
        let gate = RestoreGate::default();
        let completion = gate.begin().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                started_tx.send(()).unwrap();
                finished_tx.send(gate.wait()).unwrap();
            });
            started_rx.recv().unwrap();
            assert!(finished_rx.recv_timeout(Duration::from_millis(20)).is_err());
            drop(completion);
            assert_eq!(
                finished_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                Ok(())
            );
        });
    }

    #[test]
    fn panic_during_restore_releases_waiting_commands() {
        let gate = RestoreGate::default();
        let result = std::panic::catch_unwind(|| {
            let _completion = gate.begin().unwrap();
            panic!("fixture restore failure");
        });
        assert!(result.is_err());
        gate.wait().unwrap();
    }
}
