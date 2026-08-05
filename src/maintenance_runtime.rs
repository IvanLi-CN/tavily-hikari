use std::future::Future;
use std::sync::Arc;

use tokio::sync::{Mutex, Notify, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct MaintenanceRuntime {
    wake: Arc<Notify>,
    remote_io_slot: Arc<Semaphore>,
    maintenance_lease: Arc<Mutex<()>>,
    remote_tasks: Arc<Mutex<JoinSet<()>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceRuntimeCompletion {
    Cancelled,
    TasksExhausted,
}

impl MaintenanceRuntime {
    pub fn new() -> Self {
        Self {
            wake: Arc::new(Notify::new()),
            remote_io_slot: Arc::new(Semaphore::new(1)),
            maintenance_lease: Arc::new(Mutex::new(())),
            remote_tasks: Arc::new(Mutex::new(JoinSet::new())),
        }
    }

    pub fn wake(&self) -> Arc<Notify> {
        self.wake.clone()
    }

    pub fn try_acquire_remote_io_slot(&self) -> Option<OwnedSemaphorePermit> {
        self.remote_io_slot.clone().try_acquire_owned().ok()
    }

    pub fn try_acquire_maintenance_lease(&self) -> Option<OwnedMutexGuard<()>> {
        self.maintenance_lease.clone().try_lock_owned().ok()
    }

    pub async fn supervise(
        &self,
        cancellation: CancellationToken,
        tasks: Vec<JoinHandle<()>>,
    ) -> MaintenanceRuntimeCompletion {
        if tasks.is_empty() {
            cancellation.cancelled().await;
            self.abort_remote_tasks().await;
            return MaintenanceRuntimeCompletion::Cancelled;
        }

        let mut task_set = JoinSet::new();
        for task in tasks {
            let task_cancellation = cancellation.clone();
            task_set.spawn(async move {
                // Keep the adopted handle abort-safe if the coordinator itself is dropped.
                let mut task = tokio_util::task::AbortOnDropHandle::new(task);
                tokio::select! {
                    _ = task_cancellation.cancelled() => {
                        task.abort();
                        let _ = task.await;
                    }
                    result = &mut task => {
                        let _ = result;
                    }
                }
            });
        }

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    while task_set.join_next().await.is_some() {}
                    self.abort_remote_tasks().await;
                    return MaintenanceRuntimeCompletion::Cancelled;
                }
                result = task_set.join_next() => {
                    match result {
                        Some(_) => {}
                        None => {
                            self.abort_remote_tasks().await;
                            return MaintenanceRuntimeCompletion::TasksExhausted;
                        }
                    }
                }
            }
        }
    }

    pub async fn spawn_remote_task<F>(&self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.remote_tasks.lock().await.spawn(task);
    }

    pub async fn reap_remote_tasks(&self) {
        let mut tasks = self.remote_tasks.lock().await;
        while tasks.try_join_next().is_some() {}
    }

    pub async fn abort_remote_tasks(&self) {
        let mut tasks = self.remote_tasks.lock().await;
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }

    #[cfg(test)]
    async fn remote_task_count(&self) -> usize {
        self.remote_tasks.lock().await.len()
    }
}

impl Default for MaintenanceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn runtime_owns_independent_remote_slots_and_maintenance_leases() {
        let first = MaintenanceRuntime::new();
        let second = MaintenanceRuntime::new();

        let first_remote = first
            .try_acquire_remote_io_slot()
            .expect("first runtime remote slot");
        assert!(first.try_acquire_remote_io_slot().is_none());
        assert!(second.try_acquire_remote_io_slot().is_some());
        drop(first_remote);

        let first_lease = first
            .try_acquire_maintenance_lease()
            .expect("first runtime maintenance lease");
        assert!(first.try_acquire_maintenance_lease().is_none());
        assert!(second.try_acquire_maintenance_lease().is_some());
        drop(first_lease);
    }

    #[tokio::test]
    async fn runtime_cancellation_aborts_and_drains_adopted_tasks() {
        let runtime = MaintenanceRuntime::new();
        let cancellation = CancellationToken::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let task_stopped = stopped.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            task_stopped.store(true, Ordering::SeqCst);
        });

        cancellation.cancel();
        let completion = runtime.supervise(cancellation, vec![task]).await;

        assert_eq!(completion, MaintenanceRuntimeCompletion::Cancelled);
        assert!(!stopped.load(Ordering::SeqCst));
        assert_eq!(runtime.remote_task_count().await, 0);
    }

    #[tokio::test]
    async fn runtime_cancellation_aborts_dynamic_remote_tasks() {
        let runtime = MaintenanceRuntime::new();
        let cancellation = CancellationToken::new();
        let remote_started = Arc::new(Notify::new());
        let remote_started_signal = remote_started.clone();
        runtime
            .spawn_remote_task(async move {
                remote_started_signal.notify_one();
                tokio::time::sleep(Duration::from_secs(60)).await;
            })
            .await;
        remote_started.notified().await;

        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        cancellation.cancel();
        assert_eq!(
            runtime.supervise(cancellation, vec![task]).await,
            MaintenanceRuntimeCompletion::Cancelled
        );
        assert_eq!(runtime.remote_task_count().await, 0);
    }
}
