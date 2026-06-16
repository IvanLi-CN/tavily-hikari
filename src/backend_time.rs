use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone, Utc};
use tokio::time::Instant;

#[derive(Clone, Debug)]
pub struct BackendTime {
    wall_clock: Arc<dyn WallClock>,
}

impl BackendTime {
    pub fn system() -> Self {
        Self {
            wall_clock: Arc::new(SystemWallClock),
        }
    }

    pub fn now() -> Self {
        Self::system()
    }

    #[cfg(test)]
    pub fn manual(start: DateTime<Utc>) -> (Self, ManualBackendTime) {
        let wall_clock = Arc::new(ManualWallClock::new(start));
        (
            Self {
                wall_clock: wall_clock.clone(),
            },
            ManualBackendTime { wall_clock },
        )
    }

    #[cfg(test)]
    pub fn manual_from_ts(start_ts: i64) -> (Self, ManualBackendTime) {
        Self::manual(
            Utc.timestamp_opt(start_ts, 0)
                .single()
                .unwrap_or_else(Utc::now),
        )
    }

    pub fn now_ts(&self) -> i64 {
        self.wall_clock.now_ts()
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        self.wall_clock.now_utc()
    }

    pub fn instant_now(&self) -> Instant {
        Instant::now()
    }

    pub fn deadline_after(&self, duration: Duration) -> Instant {
        self.instant_now() + duration
    }

    pub fn local_now(&self) -> DateTime<Local> {
        self.now_utc().with_timezone(&Local)
    }

    pub fn sleep_until_local_daily_run(&self, hour: u32, minute: u32) -> Duration {
        duration_until_next_local_daily_run(self.local_now(), hour, minute)
    }

    pub async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

trait WallClock: Send + Sync + std::fmt::Debug {
    fn now_ts(&self) -> i64;

    fn now_utc(&self) -> DateTime<Utc> {
        Utc.timestamp_opt(self.now_ts(), 0)
            .single()
            .unwrap_or_else(Utc::now)
    }
}

#[derive(Debug)]
struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn now_ts(&self) -> i64 {
        Utc::now().timestamp()
    }
}

#[cfg(test)]
#[derive(Debug)]
struct ManualWallClock {
    now: std::sync::Mutex<DateTime<Utc>>,
}

#[cfg(test)]
impl ManualWallClock {
    fn new(start: DateTime<Utc>) -> Self {
        Self {
            now: std::sync::Mutex::new(start),
        }
    }

    fn current_utc(&self) -> DateTime<Utc> {
        self.now
            .lock()
            .expect("manual backend clock mutex poisoned")
            .to_owned()
    }

    fn set_now_utc(&self, now: DateTime<Utc>) {
        *self
            .now
            .lock()
            .expect("manual backend clock mutex poisoned") = now;
    }

    fn advance_wall(&self, duration: Duration) {
        let chrono_duration = chrono::Duration::from_std(duration)
            .expect("manual backend clock duration must fit chrono::Duration");
        let mut now = self
            .now
            .lock()
            .expect("manual backend clock mutex poisoned");
        *now += chrono_duration;
    }
}

#[cfg(test)]
impl WallClock for ManualWallClock {
    fn now_ts(&self) -> i64 {
        self.current_utc().timestamp()
    }

    fn now_utc(&self) -> DateTime<Utc> {
        self.current_utc()
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub struct ManualBackendTime {
    wall_clock: Arc<ManualWallClock>,
}

#[cfg(test)]
impl ManualBackendTime {
    pub fn now_ts(&self) -> i64 {
        self.wall_clock.current_utc().timestamp()
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        self.wall_clock.current_utc()
    }

    pub fn set_now_ts(&self, now_ts: i64) {
        self.set_now_utc(
            Utc.timestamp_opt(now_ts, 0)
                .single()
                .unwrap_or_else(Utc::now),
        );
    }

    pub fn set_now_utc(&self, now: DateTime<Utc>) {
        self.wall_clock.set_now_utc(now);
    }

    pub fn advance_wall(&self, duration: Duration) {
        self.wall_clock.advance_wall(duration);
    }

    pub async fn advance(&self, duration: Duration) {
        self.advance_wall(duration);
        tokio::time::advance(duration).await;
    }

    pub fn local_now(&self) -> DateTime<Local> {
        self.now_utc().with_timezone(&Local)
    }

    pub fn sleep_until_local_daily_run(&self, hour: u32, minute: u32) -> Duration {
        duration_until_next_local_daily_run(self.local_now(), hour, minute)
    }
}

pub fn duration_until_next_local_daily_run(
    now: DateTime<Local>,
    hour: u32,
    minute: u32,
) -> Duration {
    let today = now.date_naive();
    let scheduled_naive = today
        .and_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| today.and_hms_opt(6, 20, 0).expect("valid default time"));
    let scheduled_today = match Local.from_local_datetime(&scheduled_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => now,
    };
    if scheduled_today > now {
        return (scheduled_today - now)
            .to_std()
            .unwrap_or_else(|_| Duration::from_secs(0));
    }

    let tomorrow = today.succ_opt().unwrap_or_else(|| {
        today
            .checked_add_days(chrono::Days::new(1))
            .unwrap_or(today)
    });
    let next_naive = tomorrow
        .and_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| tomorrow.and_hms_opt(6, 20, 0).expect("valid default time"));
    let next = match Local.from_local_datetime(&next_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => now + chrono::Duration::hours(24),
    };
    (next - now)
        .to_std()
        .unwrap_or_else(|_| Duration::from_secs(0))
}
