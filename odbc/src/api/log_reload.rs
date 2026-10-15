use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use sf_core::config::{ConfigError, logging_config_from_ini, read_ini_snapshot};
use sf_core::env_vars::{
    SF_ODBC_INI, SNOWFLAKE_TROUBLESHOOTING_ENABLED, SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH,
};
use sf_core::logging::{LogReloadHandle, LoggingConfig};

use crate::api::ini_paths::default_paths;

pub struct LogReloadState {
    handle: LogReloadHandle,
    last_hash: AtomicU64,
    rebuild_lock: Mutex<()>,
    #[cfg(test)]
    rebuild_count: AtomicU64,
}

impl LogReloadState {
    pub fn new(handle: LogReloadHandle, initial_hash: u64) -> Self {
        Self {
            handle,
            last_hash: AtomicU64::new(initial_hash),
            rebuild_lock: Mutex::new(()),
            #[cfg(test)]
            rebuild_count: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    fn note_rebuild(&self) {
        self.rebuild_count.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn rebuild_count(&self) -> u64 {
        self.rebuild_count.load(Ordering::Relaxed)
    }
}

struct Resolved {
    config: LoggingConfig,
    source: Option<PathBuf>,
    skipped_env_ini: Option<PathBuf>,
}

fn resolve() -> Result<Resolved, ConfigError> {
    let paths = default_paths();
    let skipped_env_ini = std::env::var(SF_ODBC_INI)
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.is_file());
    let snapshot = read_ini_snapshot(&paths)?;
    let source = snapshot.source().map(Path::to_path_buf);
    let config = logging_config_from_ini(&snapshot)?;
    Ok(Resolved {
        config,
        source,
        skipped_env_ini,
    })
}

fn winning_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.is_file()).cloned()
}

fn fingerprint_hash() -> u64 {
    let mut hasher = DefaultHasher::new();
    std::env::var(SF_ODBC_INI).ok().hash(&mut hasher);
    std::env::var(SNOWFLAKE_TROUBLESHOOTING_ENABLED)
        .ok()
        .hash(&mut hasher);
    std::env::var(SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH)
        .ok()
        .hash(&mut hasher);
    match winning_path(&default_paths()) {
        Some(path) => {
            path.hash(&mut hasher);
            if let Ok(meta) = std::fs::metadata(&path) {
                meta.len().hash(&mut hasher);
                if let Ok(modified) = meta.modified()
                    && let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH)
                {
                    since_epoch.as_nanos().hash(&mut hasher);
                }
            }
        }
        None => 0u8.hash(&mut hasher),
    }
    hasher.finish()
}

fn emit_applied_line(resolved: &Resolved, troubleshooting: bool, report_dir: &Path) {
    let ini_source = resolved
        .source
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<none found>".to_string());
    tracing::info!(
        ini_source = %ini_source,
        skipped_sf_odbc_ini = ?resolved.skipped_env_ini,
        troubleshooting,
        report_dir = %report_dir.display(),
        "ODBC logging configuration applied (DSN LogLevel/LogPath/TRACING are not consulted)"
    );
    tracing::debug!(
        level = ?resolved.config.level,
        log_path = ?resolved.config.log_path,
        file = ?resolved.config.log_file_name,
        rotation = ?resolved.config.rotation,
        max_count = ?resolved.config.max_file_count,
        "resolved logging settings"
    );
}

pub fn init_state(
    handle: LogReloadHandle,
    troubleshooting: bool,
    report_dir: &Path,
) -> LogReloadState {
    match resolve() {
        Ok(resolved) => {
            emit_applied_line(&resolved, troubleshooting, report_dir);
            LogReloadState::new(handle, fingerprint_hash())
        }
        Err(_) => {
            let force_mismatch = fingerprint_hash().wrapping_add(1);
            LogReloadState::new(handle, force_mismatch)
        }
    }
}

pub(crate) fn refresh_logging() {
    let Ok(globals) = crate::api::runtime::global() else {
        return;
    };
    let Some(state) = globals.log_reload() else {
        return;
    };
    refresh_with_state(state);
}

fn refresh_with_state(state: &LogReloadState) {
    let hash = fingerprint_hash();
    if state.last_hash.load(Ordering::Relaxed) == hash {
        return;
    }
    let _rebuild = state.rebuild_lock.lock();
    if state.last_hash.load(Ordering::Relaxed) == hash {
        return;
    }
    #[cfg(test)]
    state.note_rebuild();
    let resolved = match resolve() {
        Ok(resolved) => resolved,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "sf.odbc.ini logging reload failed to resolve; keeping previous configuration"
            );
            return;
        }
    };
    match state.handle.reload(&resolved.config) {
        Ok(outcome) => {
            emit_applied_line(
                &resolved,
                outcome.troubleshooting,
                &outcome.troubleshooting_log_dir,
            );
            state.last_hash.store(hash, Ordering::Relaxed);
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "sf.odbc.ini logging reload failed; keeping previous configuration"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_core::logging::{LogManager, LoggingConfig};
    use tracing::level_filters::LevelFilter;

    fn disabled_ini(dir: &Path) -> PathBuf {
        let path = dir.join("sf.odbc.ini");
        std::fs::write(&path, "LogLevel=INFO\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }

    fn manager() -> LogManager {
        LogManager::init(LoggingConfig {
            enabled: true,
            level: LevelFilter::INFO,
            log_path: None,
            ..LoggingConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn fingerprint_stable_when_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let ini = disabled_ini(dir.path());
        temp_env::with_var(SF_ODBC_INI, Some(ini.as_os_str()), || {
            assert_eq!(fingerprint_hash(), fingerprint_hash());
        });
    }

    #[test]
    fn fingerprint_changes_when_file_edited() {
        let dir = tempfile::tempdir().unwrap();
        let ini = disabled_ini(dir.path());
        temp_env::with_var(SF_ODBC_INI, Some(ini.as_os_str()), || {
            let before = fingerprint_hash();
            std::fs::write(&ini, "LogLevel=DEBUG\nLogPath=/tmp/x\n").unwrap();
            assert_ne!(before, fingerprint_hash());
        });
    }

    #[test]
    fn fingerprint_changes_when_env_points_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.ini");
        let b = dir.path().join("b.ini");
        std::fs::write(&a, "LogLevel=INFO\n").unwrap();
        std::fs::write(&b, "LogLevel=WARN\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let with_a = temp_env::with_var(SF_ODBC_INI, Some(a.as_os_str()), fingerprint_hash);
        let with_b = temp_env::with_var(SF_ODBC_INI, Some(b.as_os_str()), fingerprint_hash);
        assert_ne!(with_a, with_b);
    }

    #[test]
    fn resolve_records_skipped_directory_env_ini() {
        let dir = tempfile::tempdir().unwrap();
        let as_dir = dir.path().join("sf.odbc.ini");
        std::fs::create_dir(&as_dir).unwrap();
        temp_env::with_var(SF_ODBC_INI, Some(as_dir.as_os_str()), || {
            let resolved = resolve().unwrap();
            assert_eq!(resolved.skipped_env_ini.as_deref(), Some(as_dir.as_path()));
            assert_ne!(resolved.source.as_deref(), Some(as_dir.as_path()));
        });
    }

    #[test]
    fn fast_path_does_not_rebuild_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let ini = disabled_ini(dir.path());
        temp_env::with_var(SF_ODBC_INI, Some(ini.as_os_str()), || {
            let mgr = manager();
            let state = LogReloadState::new(mgr.reload_handle().unwrap(), fingerprint_hash());
            refresh_with_state(&state);
            refresh_with_state(&state);
            assert_eq!(state.rebuild_count(), 0);
        });
    }

    #[test]
    fn stale_fingerprint_rebuilds_once_then_fast_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ini = disabled_ini(dir.path());
        temp_env::with_var(SF_ODBC_INI, Some(ini.as_os_str()), || {
            let mgr = manager();
            let state = LogReloadState::new(
                mgr.reload_handle().unwrap(),
                fingerprint_hash() ^ 0xdead_beef,
            );
            refresh_with_state(&state);
            refresh_with_state(&state);
            assert_eq!(state.rebuild_count(), 1);
        });
    }
}
