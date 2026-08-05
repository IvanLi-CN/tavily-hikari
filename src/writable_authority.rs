use std::future::Future;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub const WRITABLE_TENURE_CAPABILITY: &str = "writable_tenure_v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritableAuthorityPhase {
    Standby,
    Writable,
    Demoting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WritableAuthorityState {
    pub epoch: i64,
    pub phase: WritableAuthorityPhase,
}

impl WritableAuthorityState {
    pub const fn standby(epoch: i64) -> Self {
        Self {
            epoch,
            phase: WritableAuthorityPhase::Standby,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WritableRevision {
    epoch: i64,
    cancellation: CancellationToken,
}

impl WritableRevision {
    pub fn epoch(&self) -> i64 {
        self.epoch
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn is_current(&self, state: WritableAuthorityState) -> bool {
        !self.cancellation.is_cancelled()
            && state.phase == WritableAuthorityPhase::Writable
            && state.epoch == self.epoch
    }
}

struct ActiveRuntime {
    revision: WritableRevision,
    tasks: JoinSet<()>,
}

struct SupervisorState {
    authority: WritableAuthorityState,
    runtime: Option<ActiveRuntime>,
}

#[derive(Clone)]
pub struct WritableTenureSupervisor {
    state: Arc<Mutex<SupervisorState>>,
    lifecycle: Arc<Mutex<()>>,
}

impl WritableTenureSupervisor {
    pub fn restore(authority: WritableAuthorityState) -> Self {
        Self {
            state: Arc::new(Mutex::new(SupervisorState {
                authority,
                runtime: None,
            })),
            lifecycle: Arc::new(Mutex::new(())),
        }
    }

    pub async fn lifecycle_guard(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.lifecycle.clone().lock_owned().await
    }

    pub async fn state(&self) -> WritableAuthorityState {
        self.state.lock().await.authority
    }

    pub async fn restore_persisted(&self, authority: WritableAuthorityState) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if state.runtime.is_some() {
            return Err("cannot restore writable authority while a runtime is active".to_string());
        }
        state.authority = authority;
        Ok(())
    }

    pub async fn promote<F, Fut>(&self, epoch: i64, start: F) -> Result<WritableRevision, String>
    where
        F: FnOnce(WritableRevision) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.state.lock().await;
        if state.authority.phase == WritableAuthorityPhase::Demoting {
            return Err("writable authority is demoting; promotion is fail-closed".to_string());
        }
        if let Some(runtime) = &state.runtime {
            if runtime.revision.epoch == epoch {
                return Ok(runtime.revision.clone());
            }
            return Err("a different writable revision is still active".to_string());
        }
        if epoch < state.authority.epoch
            || (epoch == state.authority.epoch
                && state.authority.phase != WritableAuthorityPhase::Writable)
        {
            return Err("promotion epoch must advance monotonically".to_string());
        }
        let revision = WritableRevision {
            epoch,
            cancellation: CancellationToken::new(),
        };
        let mut tasks = JoinSet::new();
        tasks.spawn(start(revision.clone()));
        state.authority = WritableAuthorityState {
            epoch,
            phase: WritableAuthorityPhase::Writable,
        };
        state.runtime = Some(ActiveRuntime {
            revision: revision.clone(),
            tasks,
        });
        Ok(revision)
    }

    pub async fn begin_demotion(&self) -> WritableAuthorityState {
        let runtime = {
            let mut state = self.state.lock().await;
            if let Some(runtime) = &state.runtime {
                runtime.revision.cancellation.cancel();
            }
            state.authority.phase = WritableAuthorityPhase::Demoting;
            state.runtime.take()
        };
        if let Some(mut runtime) = runtime
            && tokio::time::timeout(
                std::time::Duration::from_millis(250),
                runtime.tasks.join_next(),
            )
            .await
            .is_err()
        {
            runtime.tasks.abort_all();
        }
        self.state.lock().await.authority
    }

    pub async fn finish_demotion(&self, persisted_epoch: i64) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if state.authority.phase != WritableAuthorityPhase::Demoting {
            return Err("authority is not demoting".to_string());
        }
        if persisted_epoch <= state.authority.epoch {
            return Err("demotion must persist a newer authority epoch".to_string());
        }
        state.authority = WritableAuthorityState::standby(persisted_epoch);
        Ok(())
    }
}

pub fn require_writable_tenure_capability<'a>(
    peers: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Result<(), String> {
    for (node_id, capability) in peers {
        if capability != Some(WRITABLE_TENURE_CAPABILITY) {
            return Err(format!(
                "planned HA transition requires {WRITABLE_TENURE_CAPABILITY} from peer {node_id}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn promotion_starts_exactly_one_runtime_for_a_revision() {
        let supervisor = WritableTenureSupervisor::restore(WritableAuthorityState::standby(4));
        let starts = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let starts = starts.clone();
            supervisor
                .promote(5, move |revision| async move {
                    starts.fetch_add(1, Ordering::SeqCst);
                    revision.cancellation_token().cancelled().await;
                })
                .await
                .expect("promotion");
        }
        tokio::task::yield_now().await;
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn demotion_cancels_old_revision_before_epoch_advances() {
        let supervisor = WritableTenureSupervisor::restore(WritableAuthorityState::standby(10));
        let stopped = Arc::new(AtomicUsize::new(0));
        let runtime_stopped = stopped.clone();
        let revision = supervisor
            .promote(11, move |revision| async move {
                revision.cancellation_token().cancelled().await;
                runtime_stopped.store(1, Ordering::SeqCst);
            })
            .await
            .expect("promotion");

        supervisor.begin_demotion().await;
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
        tokio::time::timeout(
            Duration::from_millis(250),
            revision.cancellation_token().cancelled(),
        )
        .await
        .expect("old remote work cancelled within budget");
        assert!(!revision.is_current(supervisor.state().await));

        supervisor.finish_demotion(12).await.expect("demotion");
        assert_eq!(
            supervisor.state().await,
            WritableAuthorityState::standby(12)
        );
        assert!(!revision.is_current(supervisor.state().await));
    }

    #[tokio::test]
    async fn restored_demoting_authority_rejects_promotion() {
        let supervisor = WritableTenureSupervisor::restore(WritableAuthorityState {
            epoch: 8,
            phase: WritableAuthorityPhase::Demoting,
        });
        let error = supervisor
            .promote(9, |_| async {})
            .await
            .expect_err("demoting restart must fail closed");
        assert!(error.contains("demoting"));
    }

    #[test]
    fn planned_transition_requires_capability_from_every_peer() {
        assert!(
            require_writable_tenure_capability([("new", Some(WRITABLE_TENURE_CAPABILITY))]).is_ok()
        );
        assert!(require_writable_tenure_capability([("old", None)]).is_err());
        assert!(require_writable_tenure_capability([("unknown", Some("unknown"))]).is_err());
    }
}
