use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use sf_core::logging::{LogManager, LoggingConfig};
use tracing::level_filters::LevelFilter;

/// Serialises the tests in this binary: each installs a thread-local dispatch
/// and calls `LogReloadHandle::reload`, which rebuilds the process-global
/// callsite interest cache. Running them concurrently would let one test's
/// rebuild observe another's dispatch.
fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn file_config(log_dir: &std::path::Path, level: LevelFilter) -> LoggingConfig {
    LoggingConfig {
        enabled: true,
        level,
        log_path: Some(log_dir.to_path_buf()),
        ..LoggingConfig::default()
    }
}

const POLL_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

fn read_dir_combined(dir: &Path) -> String {
    let mut combined = String::new();
    for entry in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        combined.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
    }
    combined
}

fn wait_for_dir_contains(dir: &Path, needle: &str) -> String {
    let deadline = Instant::now() + POLL_TIMEOUT;
    loop {
        let combined = read_dir_combined(dir);
        if combined.contains(needle) || Instant::now() >= deadline {
            return combined;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_file_contains(path: &Path, needle: &str) -> String {
    let deadline = Instant::now() + POLL_TIMEOUT;
    loop {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        if contents.contains(needle) || Instant::now() >= deadline {
            return contents;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Reloading with a new `LogPath` moves the core file sink: events after the
/// swap land in the new directory, and the old directory stops receiving them.
#[test]
fn reload_swaps_core_file_sink_to_new_path() {
    let _lock = test_lock();
    let dir = tempfile::tempdir().unwrap();
    let dir_a = dir.path().join("a");
    let dir_b = dir.path().join("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();

    let lm = LogManager::init(file_config(&dir_a, LevelFilter::INFO)).unwrap();
    let handle = lm.reload_handle().expect("init installs a reload handle");
    let _guard = tracing::dispatcher::set_default(lm.dispatch());

    tracing::info!("reload_sink_message_in_dir_a");
    let a_pre = wait_for_dir_contains(&dir_a, "reload_sink_message_in_dir_a");
    assert!(
        a_pre.contains("reload_sink_message_in_dir_a"),
        "the pre-reload event should land in dir_a; got: {a_pre}"
    );

    handle
        .reload(&file_config(&dir_b, LevelFilter::INFO))
        .unwrap();

    tracing::info!("reload_sink_message_in_dir_b");

    let b = wait_for_dir_contains(&dir_b, "reload_sink_message_in_dir_b");
    let a = read_dir_combined(&dir_a);
    assert!(
        b.contains("reload_sink_message_in_dir_b"),
        "the post-reload event should land in dir_b; got: {b}"
    );
    assert!(
        !a.contains("reload_sink_message_in_dir_b"),
        "dir_a should not receive events after the sink swapped; got: {a}"
    );
}

/// Reloading with a different level changes which events are recorded, proving
/// the swap also rebuilds the callsite interest cache.
#[test]
fn reload_changes_effective_level() {
    let _lock = test_lock();
    let dir = tempfile::tempdir().unwrap();
    let log_dir = dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();

    let lm = LogManager::init(file_config(&log_dir, LevelFilter::INFO)).unwrap();
    let handle = lm.reload_handle().unwrap();
    let _guard = tracing::dispatcher::set_default(lm.dispatch());

    tracing::debug!("level_reload_debug_at_info");
    tracing::info!("level_reload_info_at_info");

    handle
        .reload(&file_config(&log_dir, LevelFilter::DEBUG))
        .unwrap();
    tracing::debug!("level_reload_debug_at_debug");

    handle
        .reload(&file_config(&log_dir, LevelFilter::WARN))
        .unwrap();
    tracing::info!("level_reload_info_at_warn");
    tracing::warn!("level_reload_warn_at_warn");

    let combined = wait_for_dir_contains(&log_dir, "level_reload_warn_at_warn");
    assert!(
        !combined.contains("level_reload_debug_at_info"),
        "DEBUG is below the initial INFO level; got: {combined}"
    );
    assert!(
        combined.contains("level_reload_info_at_info"),
        "INFO at the initial INFO level should appear; got: {combined}"
    );
    assert!(
        combined.contains("level_reload_debug_at_debug"),
        "DEBUG after reload to DEBUG should appear; got: {combined}"
    );
    assert!(
        !combined.contains("level_reload_info_at_warn"),
        "INFO after reload to WARN should be filtered; got: {combined}"
    );
    assert!(
        combined.contains("level_reload_warn_at_warn"),
        "WARN after reload to WARN should appear, distinguishing WARN from OFF; got: {combined}"
    );
}

/// A reload whose new config cannot build its appender returns `Err` and leaves
/// the previously installed sink in place.
#[test]
fn reload_with_unbuildable_path_keeps_previous_sink() {
    let _lock = test_lock();
    let dir = tempfile::tempdir().unwrap();
    let good_dir = dir.path().join("good");
    std::fs::create_dir_all(&good_dir).unwrap();

    let lm = LogManager::init(file_config(&good_dir, LevelFilter::INFO)).unwrap();
    let handle = lm.reload_handle().unwrap();
    let _guard = tracing::dispatcher::set_default(lm.dispatch());

    tracing::info!("invalid_reload_before");
    let before = wait_for_dir_contains(&good_dir, "invalid_reload_before");
    assert!(
        before.contains("invalid_reload_before"),
        "the pre-reload event should land in the good directory; got: {before}"
    );

    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let bad_dir = blocker.join("under_a_file");
    let result = handle.reload(&file_config(&bad_dir, LevelFilter::INFO));
    assert!(
        result.is_err(),
        "reload with an unbuildable log path should error"
    );

    tracing::info!("invalid_reload_after");
    let combined = wait_for_dir_contains(&good_dir, "invalid_reload_after");
    assert!(
        combined.contains("invalid_reload_after"),
        "the previous sink must stay installed after a failed reload; got: {combined}"
    );
}

/// Reloading with troubleshooting toggled on installs the troubleshooting file
/// layer alongside the core layer; toggling it back off removes it. The
/// combined [core, troubleshooting] vector swaps as a unit.
#[test]
fn reload_toggles_troubleshooting_layer() {
    let _lock = test_lock();
    let dir = tempfile::tempdir().unwrap();
    let core_dir = dir.path().join("core");
    let ts_dir = dir.path().join("ts");
    std::fs::create_dir_all(&core_dir).unwrap();
    std::fs::create_dir_all(&ts_dir).unwrap();
    let ts_dir_str = ts_dir.to_str().unwrap().to_owned();

    let lm = temp_env::with_vars(
        [
            ("SNOWFLAKE_TROUBLESHOOTING_ENABLED", None::<&str>),
            (
                "SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH",
                Some(ts_dir_str.as_str()),
            ),
        ],
        || LogManager::init(file_config(&core_dir, LevelFilter::INFO)).unwrap(),
    );
    let handle = lm.reload_handle().unwrap();
    let _guard = tracing::dispatcher::set_default(lm.dispatch());

    let ts_log = ts_dir.join("sf_driver_troubleshooting.log");

    temp_env::with_vars(
        [
            ("SNOWFLAKE_TROUBLESHOOTING_ENABLED", Some("true")),
            (
                "SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH",
                Some(ts_dir_str.as_str()),
            ),
        ],
        || {
            let outcome = handle
                .reload(&file_config(&core_dir, LevelFilter::INFO))
                .unwrap();
            assert!(
                outcome.troubleshooting,
                "the outcome should report troubleshooting active"
            );
            tracing::info!("troubleshooting_on_message");
        },
    );

    let ts_on = wait_for_file_contains(&ts_log, "troubleshooting_on_message");
    assert!(
        ts_on.contains("troubleshooting_on_message"),
        "the troubleshooting log should capture events while enabled; got: {ts_on}"
    );

    temp_env::with_vars(
        [
            ("SNOWFLAKE_TROUBLESHOOTING_ENABLED", None::<&str>),
            (
                "SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH",
                Some(ts_dir_str.as_str()),
            ),
        ],
        || {
            let outcome = handle
                .reload(&file_config(&core_dir, LevelFilter::INFO))
                .unwrap();
            assert!(
                !outcome.troubleshooting,
                "the outcome should report troubleshooting inactive"
            );
            tracing::info!("troubleshooting_off_message");
            tracing::info!("troubleshooting_off_core_witness");
        },
    );

    let core_witness = wait_for_dir_contains(&core_dir, "troubleshooting_off_core_witness");
    assert!(
        core_witness.contains("troubleshooting_off_core_witness"),
        "the core sink should still receive events after troubleshooting is disabled; got: {core_witness}"
    );

    let ts_after = std::fs::read_to_string(&ts_log).unwrap_or_default();
    assert!(
        !ts_after.contains("troubleshooting_off_message"),
        "the troubleshooting log must stop receiving events after disable; got: {ts_after}"
    );
}

/// Reloading with the core sink disabled stops events reaching the file;
/// reloading back to an enabled config with a path resumes them, proving the
/// core writer round-trips through a `None` sink and an `OFF` level filter.
#[test]
fn reload_disables_then_reenables_core_sink() {
    let _lock = test_lock();
    let dir = tempfile::tempdir().unwrap();
    let log_dir = dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();

    let lm = LogManager::init(file_config(&log_dir, LevelFilter::INFO)).unwrap();
    let handle = lm.reload_handle().unwrap();
    let _guard = tracing::dispatcher::set_default(lm.dispatch());

    tracing::info!("core_before_disable");
    let before = wait_for_dir_contains(&log_dir, "core_before_disable");
    assert!(
        before.contains("core_before_disable"),
        "the pre-disable event should land in the core sink; got: {before}"
    );

    let disabled = LoggingConfig {
        enabled: false,
        ..file_config(&log_dir, LevelFilter::INFO)
    };
    handle.reload(&disabled).unwrap();
    tracing::info!("core_while_disabled");

    handle
        .reload(&file_config(&log_dir, LevelFilter::INFO))
        .unwrap();
    tracing::info!("core_after_reenable");

    let after = wait_for_dir_contains(&log_dir, "core_after_reenable");
    assert!(
        after.contains("core_after_reenable"),
        "events should resume after the core sink is re-enabled; got: {after}"
    );
    assert!(
        !after.contains("core_while_disabled"),
        "events emitted while the core sink was disabled must not appear; got: {after}"
    );
}

/// With troubleshooting enabled, a reload whose report directory cannot be
/// created returns `Err` and leaves the working troubleshooting sink in place
/// rather than tearing it down. Exercises the reload guard that refuses to swap
/// when a requested troubleshooting appender could not be built.
#[test]
fn reload_troubleshooting_dir_failure_keeps_sink() {
    let _lock = test_lock();
    let dir = tempfile::tempdir().unwrap();
    let core_dir = dir.path().join("core");
    let ts_dir = dir.path().join("ts");
    std::fs::create_dir_all(&core_dir).unwrap();
    std::fs::create_dir_all(&ts_dir).unwrap();
    let ts_dir_str = ts_dir.to_str().unwrap().to_owned();

    let lm = temp_env::with_vars(
        [
            ("SNOWFLAKE_TROUBLESHOOTING_ENABLED", Some("true")),
            (
                "SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH",
                Some(ts_dir_str.as_str()),
            ),
        ],
        || LogManager::init(file_config(&core_dir, LevelFilter::INFO)).unwrap(),
    );
    let handle = lm.reload_handle().unwrap();
    let _guard = tracing::dispatcher::set_default(lm.dispatch());

    let ts_log = ts_dir.join("sf_driver_troubleshooting.log");
    tracing::info!("ts_dir_fail_before");
    let before = wait_for_file_contains(&ts_log, "ts_dir_fail_before");
    assert!(
        before.contains("ts_dir_fail_before"),
        "the troubleshooting sink should capture events before the failed reload; got: {before}"
    );

    let blocker = dir.path().join("ts_blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let unbuildable = blocker.join("under_a_file");
    let unbuildable_str = unbuildable.to_str().unwrap().to_owned();

    let result = temp_env::with_vars(
        [
            ("SNOWFLAKE_TROUBLESHOOTING_ENABLED", Some("true")),
            (
                "SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH",
                Some(unbuildable_str.as_str()),
            ),
        ],
        || handle.reload(&file_config(&core_dir, LevelFilter::INFO)),
    );
    assert!(
        result.is_err(),
        "troubleshooting requested with an uncreatable report dir should error"
    );

    tracing::info!("ts_dir_fail_after");
    let after = wait_for_file_contains(&ts_log, "ts_dir_fail_after");
    assert!(
        after.contains("ts_dir_fail_after"),
        "the working troubleshooting sink must stay installed after a failed reload; got: {after}"
    );
}
