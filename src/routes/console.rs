//! `routes/console.php` — the console's command list, and the schedule.
//!
//! Laravel splits these across `routes/console.php` and
//! `App\Console\Kernel::schedule`. Both answer "what this application does when
//! nothing is asking it over HTTP", so they live together.

use std::time::Duration;

use rainier_framework::console_kernel::Console;
use rainier_framework::scheduler::Schedule;

use crate::app::console::commands::SeedCommand;

/// Every command this application answers to.
///
/// `rainier_framework::console` supplies the built-ins — `serve`, `route:list`,
/// `migrate`, `queue:work`, `schedule:run` — and your own are registered on
/// top.
pub fn commands() -> Console {
    rainier_framework::console("app").register(SeedCommand)
}

/// Everything that runs on a clock.
///
/// Driven by one system cron entry:
///
/// ```cron
/// * * * * * cd /srv/app && ./app schedule:run >> /dev/null 2>&1
/// ```
///
/// …or by `./app schedule:work` in a container with no cron.
pub fn schedule(schedule: &mut Schedule) {
    // Sessions in the database accumulate rows forever otherwise. Half an hour
    // is generous for an indexed `DELETE`; the TTL is for a run that *dies*,
    // not one that is merely slow.
    schedule
        .call("sessions:prune", |app| {
            Box::pin(async move {
                use rainier_framework::database::Database;
                use rainier_framework::session::DatabaseSessionStore;

                // Only the database store has rows to prune — the others expire
                // themselves. A task that sometimes does nothing is easier to
                // reason about than one that is sometimes absent from the schedule.
                if let Ok(database) = app.resolve::<Database>() {
                    let pruned = DatabaseSessionStore::new((*database).clone()).prune().await?;
                    if pruned > 0 {
                        tracing::info!(pruned, "expired sessions removed");
                    }
                }
                Ok(())
            })
        })
        .hourly()
        .without_overlapping(Duration::from_secs(1800));

    // Jobs held by a worker that died are invisible until something releases
    // them. Every five minutes means a crash costs minutes rather than hours.
    schedule
        .call("queue:reclaim", |app| {
            Box::pin(async move {
                use rainier_framework::database::Database;
                use rainier_framework::queue::DatabaseQueue;

                if let Ok(database) = app.resolve::<Database>() {
                    let reclaimed =
                        DatabaseQueue::new((*database).clone()).reclaim_expired().await?;
                    if reclaimed > 0 {
                        tracing::warn!(reclaimed, "jobs reclaimed from workers that died");
                    }
                }
                Ok(())
            })
        })
        .every_five_minutes()
        .without_overlapping(Duration::from_secs(300));

    // `on_one_server`, not `without_overlapping`. The risk here is not that it
    // overlaps itself — it is instant — but that three machines each send the
    // digest and every subscriber gets it three times.
    //
    // Over a `MemoryCache` that guarantees nothing, which is why
    // `AppServiceProvider::boot` warns when the cache is per-process.
    schedule
        .call("mail:weekly-digest", |_app| {
            Box::pin(async move {
                tracing::info!("the weekly digest would go out here");
                Ok(())
            })
        })
        .weekly_on(1, "09:00")
        .described_as("Send the weekly digest")
        .on_one_server();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn the_built_ins_and_our_own_are_all_registered() {
        let console = commands();

        for name in [
            "serve",
            "route:list",
            "migrate",
            "queue:work",
            "schedule:run",
            "schedule:list",
            "app:seed",
        ] {
            assert!(console.find(name).is_some(), "`{name}` should be registered");
        }
    }

    fn built() -> Schedule {
        let mut built = Schedule::new();
        schedule(&mut built);
        built
    }

    #[test]
    fn every_expression_parses() {
        // The builders are lenient mid-chain, so this is the assertion that
        // turns a typo back into a failure.
        let errors = built().errors();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn no_two_tasks_share_a_name() {
        // A name is a lock key. Two tasks sharing one would block each other,
        // and the symptom is a task that mysteriously never runs.
        assert!(built().duplicate_names().is_empty());
    }

    #[test]
    fn the_digest_is_due_on_monday_morning_and_not_otherwise() {
        let schedule = built();

        let monday_nine = Utc.with_ymd_and_hms(2026, 8, 3, 9, 0, 0).unwrap();
        let due: Vec<String> = schedule.due(monday_nine).iter().map(|t| t.name()).collect();
        assert!(due.contains(&"mail:weekly-digest".to_string()), "{due:?}");

        let tuesday_nine = Utc.with_ymd_and_hms(2026, 8, 4, 9, 0, 0).unwrap();
        let due: Vec<String> = schedule.due(tuesday_nine).iter().map(|t| t.name()).collect();
        assert!(!due.contains(&"mail:weekly-digest".to_string()), "{due:?}");
    }

    #[test]
    fn the_reclaim_runs_every_five_minutes() {
        let schedule = built();

        for minute in [0, 5, 55] {
            let at = Utc.with_ymd_and_hms(2026, 8, 3, 13, minute, 0).unwrap();
            let due: Vec<String> = schedule.due(at).iter().map(|t| t.name()).collect();
            assert!(due.contains(&"queue:reclaim".to_string()), "{minute}: {due:?}");
        }

        let at = Utc.with_ymd_and_hms(2026, 8, 3, 13, 3, 0).unwrap();
        let due: Vec<String> = schedule.due(at).iter().map(|t| t.name()).collect();
        assert!(!due.contains(&"queue:reclaim".to_string()), "{due:?}");
    }

    #[test]
    fn everything_has_a_guard() {
        // The pruning tasks are slow and contend with themselves; the digest is
        // instant and contends across machines. Getting the two guards the
        // wrong way round is the mistake this pins.
        for task in built().tasks() {
            let guarded = task.overlap_ttl().is_some() || task.is_one_server();
            assert!(guarded, "`{}` has no guard at all — is that deliberate?", task.name());
        }
    }
}
