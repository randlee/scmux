use chrono::{TimeZone, Utc};
use scmux_daemon::scheduler::should_run_now;

#[test]
fn td_05_should_run_now_true_when_cron_fires_in_window() {
    let now = Utc
        .with_ymd_and_hms(2026, 1, 1, 12, 0, 10)
        .single()
        .expect("valid datetime");
    assert!(should_run_now("0 0 12 1 1 *", &now));
}

#[test]
fn td_06_should_run_now_false_when_cron_does_not_fire_in_window() {
    let now = Utc
        .with_ymd_and_hms(2026, 1, 1, 12, 0, 10)
        .single()
        .expect("valid datetime");
    assert!(!should_run_now("0 1 12 1 1 *", &now));
}

#[test]
fn td_07_should_run_now_invalid_cron_returns_false() {
    let now = Utc::now();
    assert!(!should_run_now("not-a-cron", &now));
}
