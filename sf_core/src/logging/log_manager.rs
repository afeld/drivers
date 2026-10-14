use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use opentelemetry::trace::TracerProvider;
use tracing::Level;
use tracing::level_filters::LevelFilter;
use tracing_appender::rolling::RollingFileAppender;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, Registry, reload};

use crate::env_vars;
use crate::fs_adapter::{FsAdapter, RealFs};
use crate::telemetry::os_details::detect_os_details;
use crate::telemetry::session_telemetry::SessionTelemetry;
use crate::telemetry::snowflake_exporter::SessionRegistry;
use crate::utils::sync::RwLockRecoverExt;

use snafu::ResultExt;

use super::error::{InitSnafu, LogError, ReloadSnafu};
use super::{EmptyLayer, LoggingConfig};

type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync + 'static>;

const TROUBLESHOOTING_LOG_FILE: &str = "sf_driver_troubleshooting.log";

/// A [`MakeWriter`] whose destination can be swapped in place. Backing a file
/// sink with this lets [`LogReloadHandle::reload`] redirect the sink to a new
/// `RollingFileAppender` (or disable it) without rebuilding the layer, so the
/// layer's `FilterId` stays assigned for the life of the subscriber.
#[derive(Clone)]
struct ReloadableWriter {
    inner: Arc<RwLock<Option<Arc<RollingFileAppender>>>>,
}

impl ReloadableWriter {
    fn new(appender: Option<Arc<RollingFileAppender>>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(appender)),
        }
    }

    fn set(&self, appender: Option<Arc<RollingFileAppender>>) {
        *self.inner.write_recover() = appender;
    }
}

struct ReloadableWriterGuard {
    appender: Option<Arc<RollingFileAppender>>,
}

impl<'a> MakeWriter<'a> for ReloadableWriter {
    type Writer = ReloadableWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        ReloadableWriterGuard {
            appender: self.inner.read_recover().clone(),
        }
    }
}

impl Write for ReloadableWriterGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &self.appender {
            Some(a) => a.make_writer().write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &self.appender {
            Some(a) => a.make_writer().flush(),
            None => Ok(()),
        }
    }
}

/// Wraps the composed tracing subscriber and owns the `SdkTracerProvider`,
/// tying the OTEL exporter lifetime to the subscriber. Lives inside a
/// [`tracing::dispatcher::Dispatch`] (which `Arc`-wraps it); the provider
/// stays alive as long as any `Dispatch` clone exists.
struct DriverSubscriber {
    #[allow(dead_code)]
    telemetry_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    inner: Box<dyn tracing::Subscriber + Send + Sync>,
}

impl tracing::Subscriber for DriverSubscriber {
    fn register_callsite(
        &self,
        metadata: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        self.inner.register_callsite(metadata)
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        self.inner.max_level_hint()
    }

    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        self.inner.new_span(attrs)
    }

    fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        self.inner.record(span, values);
    }

    fn record_follows_from(&self, span: &tracing::span::Id, follows: &tracing::span::Id) {
        self.inner.record_follows_from(span, follows);
    }

    fn event_enabled(&self, event: &tracing::Event<'_>) -> bool {
        self.inner.event_enabled(event)
    }

    fn event(&self, event: &tracing::Event<'_>) {
        self.inner.event(event);
    }

    fn enter(&self, span: &tracing::span::Id) {
        self.inner.enter(span);
    }

    fn exit(&self, span: &tracing::span::Id) {
        self.inner.exit(span);
    }

    fn clone_span(&self, id: &tracing::span::Id) -> tracing::span::Id {
        self.inner.clone_span(id)
    }

    fn try_close(&self, id: tracing::span::Id) -> bool {
        self.inner.try_close(id)
    }

    fn current_span(&self) -> tracing_core::span::Current {
        self.inner.current_span()
    }

    fn on_register_dispatch(&self, subscriber: &tracing::dispatcher::Dispatch) {
        self.inner.on_register_dispatch(subscriber);
    }

    unsafe fn downcast_raw(&self, id: std::any::TypeId) -> Option<*const ()> {
        if id == std::any::TypeId::of::<Self>() {
            Some(self as *const Self as *const ())
        } else {
            // SAFETY: delegating to inner subscriber which upholds the same
            // contract — returned pointer is valid for the type identified by `id`.
            unsafe { self.inner.downcast_raw(id) }
        }
    }
}

/// Telemetry and logging state created during logging initialisation.
///
/// Call exactly one of [`LogManager::init`], [`LogManager::with_app_sink`],
/// [`LogManager::for_odbc`], or [`LogManager::for_toml`] per driver
/// lifecycle. The returned instance holds a [`tracing::dispatcher::Dispatch`]
/// that consumers install per-thread via
/// [`tracing::dispatcher::set_default`], the `SessionRegistry`, and
/// lazily-computed OS details. Inject it into [`DatabaseDriverV1`] via
/// [`DriverProviders`].
pub struct LogManager {
    dispatch: tracing::dispatcher::Dispatch,
    /// Unified per-session telemetry buffer: both the OTel span lane and the
    /// raw-log lane push into it, and it owns the shared `SessionRegistry`.
    telemetry: SessionTelemetry,
    os_details: once_cell::sync::OnceCell<Option<HashMap<String, String>>>,
    fs: Arc<dyn FsAdapter>,
    /// Process-wide default for `log_query_text` parsed from `sf.odbc.ini` /
    /// `[log]` TOML. `None` means "unset; let the registry default win".
    log_query_text: Option<bool>,
    /// Process-wide default for `log_query_parameters`. See
    /// [`Self::log_query_text`].
    log_query_parameters: Option<bool>,
    error_trace_enabled: bool,
    /// Whether troubleshooting mode is active. Resolved once at init from
    /// `SNOWFLAKE_TROUBLESHOOTING_ENABLED` env var; immutable thereafter.
    troubleshooting: bool,
    /// Resolved once at construction so all consumers (file layer, diagnostic
    /// runner) agree on the directory even if the env var changes mid-process.
    troubleshooting_log_dir: std::path::PathBuf,
    /// Handle to the hot-swappable file layers (core + troubleshooting), used
    /// to re-apply logging config without rebuilding the `Dispatch`. `None`
    /// when the subscriber is externally managed
    /// ([`Self::with_none_subscriber`]).
    reload_handle: Option<LogReloadHandle>,
}

/// Handle to the hot-swappable core and troubleshooting file sinks.
///
/// Each sink is built once into the subscriber as a `fmt` layer filtered by a
/// [`reload::Layer<LevelFilter, Registry>`]; its `FilterId` is assigned once and
/// never changes. A reload mutates only the inner values: the level filter in
/// place (which rebuilds the callsite interest cache) and the sink's
/// destination through a [`ReloadableWriter`]. The owning
/// [`tracing::dispatcher::Dispatch`] identity stays fixed, so all other layers,
/// the session registry, and the negotiated encoding are untouched. Cloning is
/// cheap (shared `Arc`s) and every clone drives the same subscriber.
#[derive(Clone)]
pub struct LogReloadHandle {
    core_writer: ReloadableWriter,
    core_level: reload::Handle<LevelFilter, Registry>,
    troubleshooting_writer: ReloadableWriter,
    troubleshooting_level: reload::Handle<LevelFilter, Registry>,
}

/// What [`LogReloadHandle::reload`] resolved from the environment and applied.
pub struct ReloadOutcome {
    pub troubleshooting: bool,
    pub troubleshooting_log_dir: std::path::PathBuf,
}

impl LogReloadHandle {
    /// Re-resolve troubleshooting from the environment, rebuild the core and
    /// troubleshooting appenders from `config`, and swap them in place.
    ///
    /// Both appenders are built before anything is swapped, so a build failure
    /// (e.g. an unwritable `log_path`) returns `Err` with the previously
    /// installed sinks left running. Reloading the level filter rebuilds the
    /// callsite interest cache, so a changed level takes effect immediately.
    pub fn reload(&self, config: &LoggingConfig) -> Result<ReloadOutcome, LogError> {
        let troubleshooting = LogManager::resolve_troubleshooting();
        let troubleshooting_log_dir = LogManager::resolve_troubleshooting_log_path();

        let core_appender = LogManager::build_core_appender(config)?;
        let troubleshooting_appender = if troubleshooting {
            LogManager::build_troubleshooting_appender(&troubleshooting_log_dir)?
        } else {
            None
        };

        if troubleshooting && troubleshooting_appender.is_none() {
            return Err(InitSnafu {
                message: format!(
                    "troubleshooting was requested but its report directory could not be created: {}",
                    troubleshooting_log_dir.display()
                ),
            }
            .build());
        }

        let core_level = LogManager::core_level_filter(config);
        let troubleshooting_level = if troubleshooting_appender.is_some() {
            LevelFilter::TRACE
        } else {
            LevelFilter::OFF
        };

        self.core_writer.set(core_appender);
        self.troubleshooting_writer.set(troubleshooting_appender);
        self.core_level.reload(core_level).context(ReloadSnafu)?;
        self.troubleshooting_level
            .reload(troubleshooting_level)
            .context(ReloadSnafu)?;

        Ok(ReloadOutcome {
            troubleshooting,
            troubleshooting_log_dir,
        })
    }
}

impl LogManager {
    /// Returns the session registry shared by both telemetry lanes.
    pub fn telemetry_sessions(&self) -> &SessionRegistry {
        self.telemetry.sessions()
    }

    /// Returns the unified per-session telemetry buffer. Wrappers forward
    /// caller-produced entries via [`SessionTelemetry::add_log`]; registry
    /// membership is the `CLIENT_TELEMETRY_ENABLED` gate.
    pub fn telemetry(&self) -> &SessionTelemetry {
        &self.telemetry
    }

    /// Flush this connection's buffered telemetry (span + raw-log entries
    /// together) during connection close, before logout. Awaits the export so it
    /// completes while session tokens are still alive (see
    /// [`SessionTelemetry::flush_connection`]).
    pub async fn flush_connection(&self, telemetry_id: u64) {
        self.telemetry.flush_connection(telemetry_id).await;
    }

    /// Lazily detects and caches OS details (e.g. `/etc/os-release` on Linux).
    pub fn os_details(&self) -> &Option<HashMap<String, String>> {
        self.os_details
            .get_or_init(|| detect_os_details(self.fs.as_ref()))
    }

    /// Process-wide default for `log_query_text`, parsed from `sf.odbc.ini`
    /// (or the `[log]` TOML section). Acts as a fallback when no
    /// per-connection setting is supplied; explicit DSN / connection-string
    /// values still win.
    pub fn log_query_text(&self) -> Option<bool> {
        self.log_query_text
    }

    /// Process-wide default for `log_query_parameters`. See
    /// [`Self::log_query_text`] for precedence semantics.
    pub fn log_query_parameters(&self) -> Option<bool> {
        self.log_query_parameters
    }

    /// Whether user-facing error messages should include the full error trace,
    /// as parsed from the INI/TOML logging config. Consumer crates read this
    /// during init to seed their own rendering-policy state.
    pub fn error_trace_enabled(&self) -> bool {
        self.error_trace_enabled
    }

    /// Whether troubleshooting mode is active (resolved at init from
    /// `SNOWFLAKE_TROUBLESHOOTING_ENABLED` env var).
    pub fn is_troubleshooting(&self) -> bool {
        self.troubleshooting
    }

    /// Returns the resolved troubleshooting log path when troubleshooting is
    /// active, `None` otherwise.  Used as a fallback by `DiagnosticRunner`
    /// so the SnowCD report lands in the same directory as troubleshooting logs.
    pub fn troubleshooting_path(&self) -> Option<std::path::PathBuf> {
        self.troubleshooting
            .then(|| self.troubleshooting_log_dir.clone())
    }

    /// The resolved troubleshooting log directory, whether or not
    /// troubleshooting is active. Mirrors the directory a
    /// [`LogReloadHandle::reload`] reports in its [`ReloadOutcome`], so a
    /// consumer can log the same destination at startup as on a later reload.
    pub fn troubleshooting_report_dir(&self) -> &std::path::Path {
        &self.troubleshooting_log_dir
    }

    /// The [`tracing::dispatcher::Dispatch`] wrapping the configured
    /// subscriber. Consumers install this per-thread via
    /// [`tracing::dispatcher::set_default`] or propagate it to spawned
    /// futures with `.with_current_subscriber()`.
    pub fn dispatch(&self) -> &tracing::dispatcher::Dispatch {
        &self.dispatch
    }

    /// Handle to the reloadable file layers, or `None` when the subscriber is
    /// externally managed. A consumer clones this to re-apply logging config
    /// (e.g. the ODBC per-connect refresh) without rebuilding the dispatch.
    pub fn reload_handle(&self) -> Option<LogReloadHandle> {
        self.reload_handle.clone()
    }

    /// Create a `LogManager` with a no-op tracing subscriber.
    ///
    /// The returned instance still provides a `SessionRegistry` and
    /// lazily-detected OS details via the given `fs`. The dispatch wraps
    /// a no-op subscriber so installing it is harmless. Use this when
    /// the real subscriber is managed externally (e.g. the host
    /// application or test harness already configures tracing).
    pub fn with_none_subscriber(fs: Arc<dyn FsAdapter>) -> Self {
        let noop = tracing::dispatcher::Dispatch::none();
        // No span processor is installed (the subscriber is external), but the
        // raw-log lane still works: `add_log` buffers against the registry.
        Self {
            dispatch: noop,
            telemetry: SessionTelemetry::new(SessionRegistry::default()),
            os_details: once_cell::sync::OnceCell::new(),
            fs,
            log_query_text: None,
            log_query_parameters: None,
            error_trace_enabled: LoggingConfig::default().error_trace_enabled,
            troubleshooting: false,
            troubleshooting_log_dir: Self::resolve_troubleshooting_log_path(),
            reload_handle: None,
        }
    }

    /// Override the cached `log_query_text` / `log_query_parameters` defaults
    /// without re-installing a tracing subscriber. Test-only ergonomics: lets
    /// integration tests build a `LogManager` via [`Self::with_none_subscriber`]
    /// and still simulate values parsed from `sf.odbc.ini`.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn with_query_log_defaults(
        mut self,
        log_query_text: Option<bool>,
        log_query_parameters: Option<bool>,
    ) -> Self {
        self.log_query_text = log_query_text;
        self.log_query_parameters = log_query_parameters;
        self
    }

    /// Initialise logging with the given config, creating a fresh
    /// `SessionRegistry` so the Snowflake telemetry layer is always
    /// installed.
    pub fn init(config: LoggingConfig) -> Result<Self, LogError> {
        let telemetry = SessionTelemetry::new(SessionRegistry::default());
        let log_query_text = config.log_query_text;
        let log_query_parameters = config.log_query_parameters;
        let error_trace_enabled = config.error_trace_enabled;
        let troubleshooting_log_dir = Self::resolve_troubleshooting_log_path();
        let troubleshooting = Self::resolve_troubleshooting();
        let (dispatch, reload_handle) = Self::try_init(
            config,
            None::<EmptyLayer>,
            Some(telemetry.clone()),
            troubleshooting,
            &troubleshooting_log_dir,
        )?;
        let lm = Self {
            dispatch,
            telemetry,
            os_details: once_cell::sync::OnceCell::new(),
            fs: Arc::new(RealFs),
            log_query_text,
            log_query_parameters,
            error_trace_enabled,
            troubleshooting,
            troubleshooting_log_dir,
            reload_handle: Some(reload_handle),
        };
        Ok(lm)
    }

    /// Initialise logging with an application-provided sink (e.g.
    /// `CallbackLayer`, `SFLoggerLayer`) that receives events in parallel
    /// with the core file layer.
    pub fn with_app_sink<L>(
        config: LoggingConfig,
        app_sink: L,
        registry: SessionRegistry,
    ) -> Result<Self, LogError>
    where
        L: Layer<Registry> + Send + Sync + 'static,
    {
        let log_query_text = config.log_query_text;
        let log_query_parameters = config.log_query_parameters;
        let error_trace_enabled = config.error_trace_enabled;
        // The caller owns `registry` (registers/deregisters sessions); the
        // telemetry buffer holds a clone of the same Arc, so lookups see them.
        let telemetry = SessionTelemetry::new(registry);
        let troubleshooting_log_dir = Self::resolve_troubleshooting_log_path();
        let troubleshooting = Self::resolve_troubleshooting();
        let (dispatch, reload_handle) = Self::try_init(
            config,
            Some(app_sink),
            Some(telemetry.clone()),
            troubleshooting,
            &troubleshooting_log_dir,
        )?;
        let lm = Self {
            dispatch,
            telemetry,
            os_details: once_cell::sync::OnceCell::new(),
            fs: Arc::new(RealFs),
            log_query_text,
            log_query_parameters,
            error_trace_enabled,
            troubleshooting,
            troubleshooting_log_dir,
            reload_handle: Some(reload_handle),
        };
        Ok(lm)
    }

    /// Factory: derive a [`LoggingConfig`] from the process-wide INI
    /// snapshot loaded via [`crate::config::load_ini_files`] and initialise
    /// logging with it.
    ///
    /// Recoverable failures degrade to [`LoggingConfig::default`] with a
    /// diagnostic on stderr: a missing snapshot (caller forgot to invoke
    /// `load_ini_files`, or that call returned an error and left the global
    /// uninitialised), a parse failure inside `logging_config_from_ini`, or
    /// a recognised key carrying an invalid value all surface as a
    /// `Failed to derive logging config from sf.odbc.ini: ...` line so the
    /// silent fallback is at least visible.
    pub fn for_odbc() -> Option<Self> {
        let config = match crate::config::get_ini_config() {
            Some(ini) => crate::config::logging_config_from_ini(ini).unwrap_or_else(|e| {
                eprintln!(
                    "Failed to derive logging config from sf.odbc.ini: {e:?}; using defaults"
                );
                LoggingConfig::default()
            }),
            None => {
                // The ODBC wrapper is expected to call `load_ini_files` before
                // `for_odbc`. Reaching this arm means either the wrapper
                // skipped that step or `load_ini_files` itself failed; the
                // wrapper already emits its own diagnostic in the latter case,
                // so a second line here is redundant but harmless.
                eprintln!(
                    "No sf.odbc.ini snapshot available (sf_core::config::load_ini_files \
                     was not called or did not complete successfully); using default \
                     logging configuration"
                );
                LoggingConfig::default()
            }
        };
        match Self::init(config) {
            Ok(lm) => Some(lm),
            Err(e) => {
                eprintln!("Failed to initialize logging: {e:?}");
                None
            }
        }
    }

    /// Factory: load `[log]` section from `config.toml`, falling back to
    /// defaults.
    pub fn for_toml() -> Option<Self> {
        let config = match crate::config::config_manager::load_config_section(
            "log",
            crate::config::toml_loader::FilePermissionCheck::Enabled,
        ) {
            Ok(Some(section)) => crate::config::logging_config_from_toml_section(&section),
            _ => LoggingConfig::default(),
        };
        match Self::init(config) {
            Ok(lm) => Some(lm),
            Err(e) => {
                eprintln!("Failed to initialize logging: {e:?}");
                None
            }
        }
    }

    fn resolve_troubleshooting() -> bool {
        std::env::var(env_vars::SNOWFLAKE_TROUBLESHOOTING_ENABLED)
            .is_ok_and(|v| crate::utils::parse_bool_token(&v) == Some(true))
    }

    fn resolve_troubleshooting_log_path() -> std::path::PathBuf {
        std::env::var(env_vars::SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()))
    }

    fn try_init<L>(
        config: LoggingConfig,
        app_sink: Option<L>,
        telemetry: Option<SessionTelemetry>,
        troubleshooting: bool,
        troubleshooting_log_dir: &std::path::Path,
    ) -> Result<(tracing::dispatcher::Dispatch, LogReloadHandle), LogError>
    where
        L: Layer<Registry> + Send + Sync + 'static,
    {
        let (reloadable_layers, reload_handle) =
            Self::build_reloadable(&config, troubleshooting, troubleshooting_log_dir)?;

        let mut layers: Vec<BoxedLayer> = reloadable_layers;

        if let Some(sink) = app_sink {
            layers.push(sink.boxed());
        }

        if config.open_telemetry {
            layers.push(OpenTelemetryLayer::new(super::opentelemetry::init_tracer()?).boxed());
        }

        let (snowflake_layer, provider) = if let Some(telemetry) = telemetry {
            let processor =
                crate::telemetry::snowflake_processor::SnowflakeSpanProcessor::new(telemetry);
            let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_span_processor(processor)
                .build();
            let tracer = provider.tracer("snowflake.telemetry");
            // Restrict the OpenTelemetryLayer to spans emitted by sf_core itself.
            // Without this filter every span from tokio, hyper, tower, tonic, …
            // would flow through `SnowflakeSpanProcessor::on_end`, take the
            // per-session buffer mutex, scan attributes for `snowflake.session.id`
            // (always absent), and be dropped — pure overhead on the hot path.
            let layer = OpenTelemetryLayer::new(tracer).with_filter(
                tracing_subscriber::filter::Targets::new().with_target("sf_core", Level::TRACE),
            );
            (Some(layer), Some(provider))
        } else {
            (None, None)
        };

        if let Some(layer) = snowflake_layer {
            layers.push(layer.boxed());
        }

        if config.stderr {
            layers.push(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_filter(LevelFilter::ERROR)
                    .boxed(),
            );
        }

        #[cfg(feature = "perf_timing")]
        layers.push(crate::perf_timing::create_perf_layer().boxed());

        let subscriber = Registry::default().with(layers);

        let driver_subscriber = DriverSubscriber {
            telemetry_provider: provider,
            inner: Box::new(subscriber),
        };

        Ok((
            tracing::dispatcher::Dispatch::new(driver_subscriber),
            reload_handle,
        ))
    }

    /// Builds the two hot-swappable file layers (core, then troubleshooting) and
    /// the [`LogReloadHandle`] that drives them. Both layers are always built so
    /// their `FilterId`s are assigned once at subscriber-build time; a disabled
    /// sink is expressed as a `None` writer and an `OFF` level filter rather than
    /// an absent layer, so [`LogReloadHandle::reload`] can enable it later
    /// without a new `FilterId`.
    fn build_reloadable(
        config: &LoggingConfig,
        troubleshooting: bool,
        troubleshooting_log_dir: &std::path::Path,
    ) -> Result<(Vec<BoxedLayer>, LogReloadHandle), LogError> {
        let core_appender = Self::build_core_appender(config)?;
        if config.max_file_size.is_some() {
            eprintln!(
                "WARNING: max_file_size is configured but size-based log rotation is not \
                 yet supported; the setting will be ignored"
            );
        }
        let core_writer = ReloadableWriter::new(core_appender);
        let (core_filter, core_level): (
            reload::Layer<LevelFilter, Registry>,
            reload::Handle<LevelFilter, Registry>,
        ) = reload::Layer::new(Self::core_level_filter(config));
        let core_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(core_writer.clone())
            .with_filter(core_filter)
            .boxed();

        let troubleshooting_appender = if troubleshooting {
            Self::build_troubleshooting_appender(troubleshooting_log_dir)?
        } else {
            None
        };
        let troubleshooting_level_init = if troubleshooting_appender.is_some() {
            LevelFilter::TRACE
        } else {
            LevelFilter::OFF
        };
        let troubleshooting_writer = ReloadableWriter::new(troubleshooting_appender);
        let (troubleshooting_filter, troubleshooting_level): (
            reload::Layer<LevelFilter, Registry>,
            reload::Handle<LevelFilter, Registry>,
        ) = reload::Layer::new(troubleshooting_level_init);
        let troubleshooting_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(troubleshooting_writer.clone())
            .with_filter(troubleshooting_filter)
            .boxed();

        Ok((
            vec![core_layer, troubleshooting_layer],
            LogReloadHandle {
                core_writer,
                core_level,
                troubleshooting_writer,
                troubleshooting_level,
            },
        ))
    }

    /// The level filter for the core sink: `config.level` when logging is
    /// enabled and a path is set, `OFF` otherwise (the sink is still present so
    /// a later reload can raise it).
    fn core_level_filter(config: &LoggingConfig) -> LevelFilter {
        if config.enabled && config.log_path.is_some() {
            config.level
        } else {
            LevelFilter::OFF
        }
    }

    /// Pre-creates the troubleshooting log file with owner-only (`0o600`) permissions on Unix
    fn ensure_troubleshooting_log_permissions(log_file: &std::path::Path) -> Result<(), LogError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(log_file)
                .map_err(|e| {
                    InitSnafu {
                        message: format!(
                            "Failed to create troubleshooting log file {}: {e}",
                            log_file.display()
                        ),
                    }
                    .build()
                })?;
            // O_CREAT only sets mode on new files; remediate stale files too.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| {
                    InitSnafu {
                        message: format!(
                            "Failed to set troubleshooting log permissions on {}: {e}",
                            log_file.display()
                        ),
                    }
                    .build()
                })?;
        }
        #[cfg(not(unix))]
        {
            let _ = log_file;
        }
        Ok(())
    }

    /// Builds the troubleshooting appender, pre-creating its directory and log
    /// file. A directory that cannot be created yields `Ok(None)` (the sink is
    /// left disabled) after a stderr diagnostic; a failed appender build or a
    /// permissions failure returns `Err`.
    fn build_troubleshooting_appender(
        log_path: &std::path::Path,
    ) -> Result<Option<Arc<RollingFileAppender>>, LogError> {
        if let Err(e) = std::fs::create_dir_all(log_path) {
            eprintln!(
                "SNOWFLAKE_TROUBLESHOOTING_REPORT_PATH: failed to create log directory {}: {e}",
                log_path.display()
            );
            return Ok(None);
        }

        let log_file = log_path.join(TROUBLESHOOTING_LOG_FILE);
        Self::ensure_troubleshooting_log_permissions(&log_file)?;

        let appender = RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::NEVER)
            .filename_prefix(TROUBLESHOOTING_LOG_FILE)
            .build(log_path)
            .map_err(|e| {
                InitSnafu {
                    message: format!("Failed to create troubleshooting log appender: {e}"),
                }
                .build()
            })?;

        Ok(Some(Arc::new(appender)))
    }

    /// Builds the core file appender, or `Ok(None)` when logging is disabled or
    /// no `log_path` is set.
    fn build_core_appender(
        config: &LoggingConfig,
    ) -> Result<Option<Arc<RollingFileAppender>>, LogError> {
        if !config.enabled {
            return Ok(None);
        }

        let Some(log_path) = config.log_path.as_ref() else {
            return Ok(None);
        };

        let file_name = config.log_file_name.as_deref().unwrap_or("sf_driver.log");

        let effective_rotation =
            if config.max_file_count.is_some() && config.rotation == super::LogRotation::Never {
                super::LogRotation::Daily
            } else {
                config.rotation
            };

        let mut builder = RollingFileAppender::builder()
            .rotation(effective_rotation.to_appender_rotation())
            .filename_prefix(file_name);

        if let Some(count) = config.max_file_count {
            builder = builder.max_log_files(count as usize);
        }

        let appender = builder.build(log_path).map_err(|e| {
            InitSnafu {
                message: format!("Failed to create log appender: {e}"),
            }
            .build()
        })?;

        Ok(Some(Arc::new(appender)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn troubleshooting_log_file_has_mode_600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        assert!(
            LogManager::build_troubleshooting_appender(dir.path())
                .unwrap()
                .is_some()
        );

        let log_file = dir.path().join(TROUBLESHOOTING_LOG_FILE);
        assert!(
            log_file.exists(),
            "troubleshooting log should be pre-created"
        );
        let mode = std::fs::metadata(&log_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn troubleshooting_log_remediates_stale_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let log_file = dir.path().join(TROUBLESHOOTING_LOG_FILE);
        std::fs::write(&log_file, "stale\n").unwrap();
        std::fs::set_permissions(&log_file, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(
            LogManager::build_troubleshooting_appender(dir.path())
                .unwrap()
                .is_some()
        );

        let mode = std::fs::metadata(&log_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "stale permissions should be remediated to 0o600"
        );
    }

    #[test]
    fn build_core_appender_disabled_returns_none() {
        let config = LoggingConfig {
            enabled: false,
            ..LoggingConfig::default()
        };
        assert!(LogManager::build_core_appender(&config).unwrap().is_none());
        assert_eq!(LogManager::core_level_filter(&config), LevelFilter::OFF);
    }

    #[test]
    fn build_core_appender_no_path_returns_none() {
        let config = LoggingConfig {
            enabled: true,
            log_path: None,
            ..LoggingConfig::default()
        };
        assert!(LogManager::build_core_appender(&config).unwrap().is_none());
        assert_eq!(LogManager::core_level_filter(&config), LevelFilter::OFF);
    }

    #[test]
    fn build_core_appender_with_path_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let config = LoggingConfig {
            enabled: true,
            log_path: Some(dir.path().to_path_buf()),
            ..LoggingConfig::default()
        };
        assert!(LogManager::build_core_appender(&config).unwrap().is_some());
    }

    #[test]
    fn build_core_appender_with_custom_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let config = LoggingConfig {
            enabled: true,
            log_path: Some(dir.path().to_path_buf()),
            log_file_name: Some("custom.log".to_string()),
            ..LoggingConfig::default()
        };
        assert!(
            LogManager::build_core_appender(&config).unwrap().is_some(),
            "build_core_appender should succeed with a custom file name"
        );
    }

    #[test]
    fn core_level_filter_tracks_config_level_when_enabled_with_path() {
        let dir = tempfile::tempdir().unwrap();
        for level in [
            LevelFilter::OFF,
            LevelFilter::ERROR,
            LevelFilter::WARN,
            LevelFilter::INFO,
            LevelFilter::DEBUG,
            LevelFilter::TRACE,
        ] {
            let config = LoggingConfig {
                enabled: true,
                log_path: Some(dir.path().to_path_buf()),
                level,
                ..LoggingConfig::default()
            };
            assert!(LogManager::build_core_appender(&config).unwrap().is_some());
            assert_eq!(LogManager::core_level_filter(&config), level);
        }
    }

    #[test]
    fn with_none_subscriber_log_query_defaults_are_none() {
        let lm = LogManager::with_none_subscriber(Arc::new(RealFs));
        assert_eq!(lm.log_query_text(), None);
        assert_eq!(lm.log_query_parameters(), None);
    }

    #[test]
    fn with_query_log_defaults_overrides_values() {
        let lm = LogManager::with_none_subscriber(Arc::new(RealFs))
            .with_query_log_defaults(Some(true), Some(false));
        assert_eq!(lm.log_query_text(), Some(true));
        assert_eq!(lm.log_query_parameters(), Some(false));
    }

    #[test]
    fn with_query_log_defaults_supports_partial_set() {
        let lm = LogManager::with_none_subscriber(Arc::new(RealFs))
            .with_query_log_defaults(Some(true), None);
        assert_eq!(lm.log_query_text(), Some(true));
        assert_eq!(lm.log_query_parameters(), None);
    }

    #[test]
    fn default_config_is_enabled_info_no_path() {
        use tracing::level_filters::LevelFilter;

        let config = LoggingConfig::default();
        assert!(config.enabled, "default config should be enabled");
        assert_eq!(
            config.level,
            LevelFilter::INFO,
            "default level should be INFO"
        );
        assert!(config.log_path.is_none(), "default log_path should be None");
        assert!(
            config.log_file_name.is_none(),
            "default log_file_name should be None"
        );
        assert!(
            !config.open_telemetry,
            "default opentelemetry should be false"
        );
    }

    #[test]
    fn with_none_subscriber_exposes_session_registry() {
        let lm = LogManager::with_none_subscriber(Arc::new(crate::fs_adapter::RealFs));
        let guard = lm.telemetry_sessions().read().unwrap();
        assert!(guard.is_empty(), "session registry should start empty");
    }

    #[test]
    fn with_none_subscriber_has_no_reload_handle() {
        let lm = LogManager::with_none_subscriber(Arc::new(RealFs));
        assert!(lm.reload_handle().is_none());
    }

    #[tokio::test]
    async fn flush_connection_is_noop_when_nothing_buffered() {
        let lm = LogManager::with_none_subscriber(Arc::new(crate::fs_adapter::RealFs));
        // No span processor is installed and nothing is buffered — must not panic.
        lm.flush_connection(42).await;
    }
}
