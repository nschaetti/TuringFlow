//! Production-grade structured logging and distributed tracing primitives.
//!
//! The logger is designed for multi-agent and distributed execution:
//! - asynchronous non-blocking ingestion,
//! - JSON file output and table console output,
//! - runtime level overrides,
//! - sampling and redaction,
//! - rotation/compression,
//! - metrics hooks,
//! - OpenTelemetry-compatible record projection.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::{self, create_dir_all, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// Severity level attached to one log event.
pub enum LogLevel {
    /// Most verbose diagnostic level.
    Trace,
    /// Developer-oriented debugging information.
    Debug,
    /// Standard operational event.
    Info,
    /// Warning about degraded or unexpected behavior.
    Warn,
    /// Error event requiring action.
    Error,
    /// Fatal unrecoverable failure.
    Fatal,
}

impl LogLevel {
    /// Returns the canonical uppercase token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }

    /// Parses a textual level name.
    ///
    /// Unknown values fallback to `INFO`.
    pub fn parse(name: &str) -> Self {
        match name {
            "trace" | "TRACE" => Self::Trace,
            "debug" | "DEBUG" => Self::Debug,
            "warn" | "WARN" => Self::Warn,
            "error" | "ERROR" => Self::Error,
            "fatal" | "FATAL" => Self::Fatal,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Event category used for indexing and filtering.
pub enum EventType {
    /// Core service events.
    System,
    /// Network and transport events.
    Network,
    /// LLM call lifecycle events.
    LlmCall,
    /// Tool call lifecycle events.
    ToolCall,
    /// Memory read events.
    MemoryRead,
    /// Memory write events.
    MemoryWrite,
    /// Performance and timing events.
    Performance,
    /// Error-rich events.
    Error,
    /// Security and policy events.
    Security,
}

impl EventType {
    /// Returns the canonical uppercase token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "SYSTEM",
            Self::Network => "NETWORK",
            Self::LlmCall => "LLM_CALL",
            Self::ToolCall => "TOOL_CALL",
            Self::MemoryRead => "MEMORY_READ",
            Self::MemoryWrite => "MEMORY_WRITE",
            Self::Performance => "PERFORMANCE",
            Self::Error => "ERROR",
            Self::Security => "SECURITY",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
/// Sampling rates for non-error levels.
///
/// Each value must be in the inclusive range `[0.0, 1.0]`.
pub struct SamplingConfig {
    /// Emission ratio for `INFO` records.
    pub info_rate: f64,
    /// Emission ratio for `DEBUG` records.
    pub debug_rate: f64,
    /// Emission ratio for `TRACE` records.
    pub trace_rate: f64,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            info_rate: 1.0,
            debug_rate: 0.2,
            trace_rate: 0.05,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
/// Rotation and compression policy for NDJSON file output.
pub struct RotationConfig {
    /// Maximum file size before rotation (bytes).
    pub max_bytes: u64,
    /// Number of rotated files kept on disk.
    pub max_files: usize,
    /// Enables gzip compression for rotated segments.
    pub compress: bool,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            max_bytes: 50 * 1024 * 1024,
            max_files: 10,
            compress: true,
        }
    }
}

/// Hook trait for exporting logger internals to metrics backends.
pub trait MetricsHook: Send + Sync {
    /// Increments a named counter.
    fn increment_counter(&self, _name: &str, _value: u64) {}
    /// Observes one histogram value.
    fn observe_histogram(&self, _name: &str, _value: f64) {}
    /// Sets an absolute gauge value.
    fn set_gauge(&self, _name: &str, _value: f64) {}
}

#[derive(Debug)]
/// No-op [`MetricsHook`] implementation.
pub struct NoopMetricsHook;

impl MetricsHook for NoopMetricsHook {}

#[derive(Clone)]
/// Logger runtime configuration.
pub struct LoggerConfig {
    /// Service name injected in each record.
    pub service: String,
    /// Node id injected in each record.
    pub node_id: String,
    /// Baseline minimum level.
    pub min_level: LogLevel,
    /// Enables aligned table output on stdout.
    pub console_table: bool,
    /// Optional NDJSON file path.
    pub file_path: Option<String>,
    /// Async queue capacity for non-blocking producers.
    pub queue_capacity: usize,
    /// Sampling policy for low-priority levels.
    pub sampling: SamplingConfig,
    /// Rotation policy for log file output.
    pub rotation: RotationConfig,
    /// Optional metrics sink.
    pub metrics_hook: Option<Arc<dyn MetricsHook>>,
}

impl std::fmt::Debug for LoggerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoggerConfig")
            .field("service", &self.service)
            .field("node_id", &self.node_id)
            .field("min_level", &self.min_level)
            .field("console_table", &self.console_table)
            .field("file_path", &self.file_path)
            .field("queue_capacity", &self.queue_capacity)
            .field("sampling", &self.sampling)
            .field("rotation", &self.rotation)
            .finish_non_exhaustive()
    }
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            service: "turingflow".to_string(),
            node_id: "unknown".to_string(),
            min_level: LogLevel::Info,
            console_table: true,
            file_path: Some("logs/turingflowd.log".to_string()),
            queue_capacity: 4096,
            sampling: SamplingConfig::default(),
            rotation: RotationConfig::default(),
            metrics_hook: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
/// Canonical structured log event payload.
pub struct LogRecord {
    /// UTC timestamp in RFC3339 with milliseconds.
    pub timestamp: String,
    /// Severity as uppercase token.
    pub level: String,
    /// Source service name.
    pub service: String,
    /// Optional source agent id.
    pub agent_id: Option<String>,
    /// Source node id.
    pub node_id: String,
    /// End-to-end trace id.
    pub trace_id: String,
    /// Current span id.
    pub span_id: String,
    /// Parent span id when available.
    pub parent_span_id: Option<String>,
    /// Event category.
    pub event_type: String,
    /// Human-readable summary.
    pub message: String,
    /// Structured metadata payload.
    pub context: Map<String, Value>,
    /// Optional execution duration.
    pub duration_ms: Option<u64>,
}

impl LogRecord {
    /// Projects the record to an OpenTelemetry-compatible JSON structure.
    pub fn to_otel_log_json(&self) -> Value {
        json!({
            "time_unix_nano": self.timestamp,
            "severity_text": self.level,
            "body": self.message,
            "trace_id": self.trace_id,
            "span_id": self.span_id,
            "attributes": {
                "service.name": self.service,
                "node.id": self.node_id,
                "agent.id": self.agent_id,
                "event.type": self.event_type,
                "context": self.context,
                "duration_ms": self.duration_ms,
            }
        })
    }
}

#[derive(Debug, Clone)]
/// Distributed trace context propagated between components.
pub struct TraceContext {
    /// Global trace identifier.
    pub trace_id: String,
    /// Current span identifier.
    pub span_id: String,
    /// Parent span identifier.
    pub parent_span_id: Option<String>,
}

impl TraceContext {
    /// Creates a new root trace context.
    pub fn root() -> Self {
        Self {
            trace_id: next_hex_id_128(),
            span_id: next_hex_id_64(),
            parent_span_id: None,
        }
    }

    /// Creates a context from an existing trace id.
    ///
    /// A new span id is generated and no parent is set.
    pub fn from_trace_id(trace_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: next_hex_id_64(),
            parent_span_id: None,
        }
    }

    /// Creates a child context preserving the current trace id.
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: next_hex_id_64(),
            parent_span_id: Some(self.span_id.clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct LevelOverride {
    level: LogLevel,
    expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct RuntimeLevelState {
    global_level: LogLevel,
    agent_levels: HashMap<String, LevelOverride>,
    trace_levels: HashMap<String, LevelOverride>,
}

#[derive(Debug, Clone, Serialize)]
/// Runtime snapshot of all active level overrides.
pub struct RuntimeLevelSnapshot {
    /// Active global level.
    pub global_level: String,
    /// Active per-agent overrides.
    pub agent_levels: HashMap<String, RuntimeLevelRule>,
    /// Active per-trace overrides.
    pub trace_levels: HashMap<String, RuntimeLevelRule>,
}

#[derive(Debug, Clone, Serialize)]
/// One runtime level rule.
pub struct RuntimeLevelRule {
    /// Override level.
    pub level: String,
    /// Expiration instant (epoch ms), if temporary.
    pub expires_at_ms: Option<i64>,
}

enum WorkerMsg {
    Record(LogRecord),
}

/// Thread-safe asynchronous logger with runtime controls.
pub struct Logger {
    cfg: LoggerConfig,
    tx: SyncSender<WorkerMsg>,
    dropped: AtomicU64,
    queue_depth: Arc<AtomicU64>,
    levels: Arc<RwLock<RuntimeLevelState>>,
    metrics_hook: Arc<dyn MetricsHook>,
}

impl Logger {
    /// Constructs and starts the logging worker thread.
    pub fn new(cfg: LoggerConfig) -> std::io::Result<Arc<Self>> {
        let capacity = cfg.queue_capacity.max(256);
        let (tx, rx) = sync_channel(capacity);
        let queue_depth = Arc::new(AtomicU64::new(0));
        let metrics_hook: Arc<dyn MetricsHook> = cfg
            .metrics_hook
            .clone()
            .unwrap_or_else(|| Arc::new(NoopMetricsHook));
        let logger = Arc::new(Self {
            cfg: cfg.clone(),
            tx,
            dropped: AtomicU64::new(0),
            queue_depth: queue_depth.clone(),
            levels: Arc::new(RwLock::new(RuntimeLevelState {
                global_level: cfg.min_level,
                agent_levels: HashMap::new(),
                trace_levels: HashMap::new(),
            })),
            metrics_hook: metrics_hook.clone(),
        });
        spawn_worker(cfg, rx, queue_depth, metrics_hook)?;
        Ok(logger)
    }

    #[allow(clippy::too_many_arguments)]
    /// Emits one structured record.
    ///
    /// This call is non-blocking for producers. If the queue is full,
    /// the record is dropped and counted.
    pub fn log(
        &self,
        level: LogLevel,
        event_type: EventType,
        message: impl Into<String>,
        trace: Option<&TraceContext>,
        agent_id: Option<&str>,
        context: Value,
        duration_ms: Option<u64>,
    ) {
        let trace_id = trace.map(|ctx| ctx.trace_id.as_str());
        if !self.should_emit(level, trace_id, agent_id) {
            return;
        }

        let trace = trace.cloned().unwrap_or_else(TraceContext::root);
        let redacted = redact_json(context);
        let context_obj = match redacted {
            Value::Object(map) => map,
            value => {
                let mut map = Map::new();
                map.insert("value".to_string(), value);
                map
            }
        };

        let record = LogRecord {
            timestamp: now_rfc3339_millis(),
            level: level.as_str().to_string(),
            service: self.cfg.service.clone(),
            agent_id: agent_id.map(ToString::to_string),
            node_id: self.cfg.node_id.clone(),
            trace_id: trace.trace_id,
            span_id: trace.span_id,
            parent_span_id: trace.parent_span_id,
            event_type: event_type.as_str().to_string(),
            message: message.into(),
            context: context_obj,
            duration_ms,
        };

        match self.tx.try_send(WorkerMsg::Record(record)) {
            Ok(()) => {
                let depth = self.queue_depth.fetch_add(1, Ordering::Relaxed) + 1;
                self.metrics_hook.set_gauge("log_queue_depth", depth as f64);
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                self.metrics_hook.increment_counter("logs_dropped_total", 1);
            }
        }
    }

    /// Returns the number of dropped records.
    pub fn dropped_logs(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Returns current queue depth.
    pub fn queue_depth(&self) -> u64 {
        self.queue_depth.load(Ordering::Relaxed)
    }

    /// Sets the active global minimum level.
    pub fn set_global_level(&self, level: LogLevel) {
        if let Ok(mut levels) = self.levels.write() {
            levels.global_level = level;
        }
    }

    /// Resets global level to the configured baseline.
    pub fn reset_global_level(&self) {
        if let Ok(mut levels) = self.levels.write() {
            levels.global_level = self.cfg.min_level;
        }
    }

    /// Sets a level override for one agent.
    ///
    /// If `ttl_ms` is set, the override expires automatically.
    pub fn set_agent_level(
        &self,
        agent_id: impl Into<String>,
        level: LogLevel,
        ttl_ms: Option<u64>,
    ) {
        let expires_at_ms =
            ttl_ms.map(|ttl| now_epoch_ms().saturating_add(ttl.min(i64::MAX as u64) as i64));
        if let Ok(mut levels) = self.levels.write() {
            levels.agent_levels.insert(
                agent_id.into(),
                LevelOverride {
                    level,
                    expires_at_ms,
                },
            );
        }
    }

    /// Sets a level override for one trace.
    ///
    /// Trace overrides take precedence over agent/global levels.
    pub fn set_trace_level(
        &self,
        trace_id: impl Into<String>,
        level: LogLevel,
        ttl_ms: Option<u64>,
    ) {
        let expires_at_ms =
            ttl_ms.map(|ttl| now_epoch_ms().saturating_add(ttl.min(i64::MAX as u64) as i64));
        if let Ok(mut levels) = self.levels.write() {
            levels.trace_levels.insert(
                trace_id.into(),
                LevelOverride {
                    level,
                    expires_at_ms,
                },
            );
        }
    }

    /// Clears one agent-level override.
    pub fn clear_agent_level(&self, agent_id: &str) {
        if let Ok(mut levels) = self.levels.write() {
            levels.agent_levels.remove(agent_id);
        }
    }

    /// Clears one trace-level override.
    pub fn clear_trace_level(&self, trace_id: &str) {
        if let Ok(mut levels) = self.levels.write() {
            levels.trace_levels.remove(trace_id);
        }
    }

    /// Returns a snapshot of active levels and overrides.
    pub fn level_snapshot(&self) -> RuntimeLevelSnapshot {
        if let Ok(mut levels) = self.levels.write() {
            let now = now_epoch_ms();
            levels
                .agent_levels
                .retain(|_, rule| !is_expired(rule.expires_at_ms, now));
            levels
                .trace_levels
                .retain(|_, rule| !is_expired(rule.expires_at_ms, now));
            return RuntimeLevelSnapshot {
                global_level: levels.global_level.as_str().to_string(),
                agent_levels: levels
                    .agent_levels
                    .iter()
                    .map(|(key, rule)| {
                        (
                            key.clone(),
                            RuntimeLevelRule {
                                level: rule.level.as_str().to_string(),
                                expires_at_ms: rule.expires_at_ms,
                            },
                        )
                    })
                    .collect(),
                trace_levels: levels
                    .trace_levels
                    .iter()
                    .map(|(key, rule)| {
                        (
                            key.clone(),
                            RuntimeLevelRule {
                                level: rule.level.as_str().to_string(),
                                expires_at_ms: rule.expires_at_ms,
                            },
                        )
                    })
                    .collect(),
            };
        }

        RuntimeLevelSnapshot {
            global_level: self.cfg.min_level.as_str().to_string(),
            agent_levels: HashMap::new(),
            trace_levels: HashMap::new(),
        }
    }

    fn should_emit(&self, level: LogLevel, trace_id: Option<&str>, agent_id: Option<&str>) -> bool {
        let effective_level = self.effective_min_level(trace_id, agent_id);
        if level < effective_level {
            return false;
        }

        match level {
            LogLevel::Error | LogLevel::Warn | LogLevel::Fatal => true,
            LogLevel::Info => sample(self.cfg.sampling.info_rate, trace_id),
            LogLevel::Debug => sample(self.cfg.sampling.debug_rate, trace_id),
            LogLevel::Trace => sample(self.cfg.sampling.trace_rate, trace_id),
        }
    }

    fn effective_min_level(&self, trace_id: Option<&str>, agent_id: Option<&str>) -> LogLevel {
        if let Ok(mut levels) = self.levels.write() {
            let now = now_epoch_ms();
            levels
                .agent_levels
                .retain(|_, rule| !is_expired(rule.expires_at_ms, now));
            levels
                .trace_levels
                .retain(|_, rule| !is_expired(rule.expires_at_ms, now));

            if let Some(trace_id) = trace_id {
                if let Some(rule) = levels.trace_levels.get(trace_id) {
                    return rule.level;
                }
            }
            if let Some(agent_id) = agent_id {
                if let Some(rule) = levels.agent_levels.get(agent_id) {
                    return rule.level;
                }
            }
            return levels.global_level;
        }
        self.cfg.min_level
    }
}

fn is_expired(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    expires_at_ms.is_some_and(|ts| ts <= now_ms)
}

fn spawn_worker(
    cfg: LoggerConfig,
    rx: Receiver<WorkerMsg>,
    queue_depth: Arc<AtomicU64>,
    metrics_hook: Arc<dyn MetricsHook>,
) -> std::io::Result<()> {
    let mut file_state = if let Some(path) = cfg.file_path.as_deref() {
        Some(LogFileState::new(path, cfg.rotation.clone())?)
    } else {
        None
    };

    std::thread::Builder::new()
        .name("turingflow-log-worker".to_string())
        .spawn(move || {
            if cfg.console_table {
                print_console_header();
            }

            while let Ok(msg) = rx.recv() {
                let remaining = queue_depth
                    .fetch_sub(1, Ordering::Relaxed)
                    .saturating_sub(1);
                metrics_hook.set_gauge("log_queue_depth", remaining as f64);
                match msg {
                    WorkerMsg::Record(record) => {
                        let write_start = std::time::Instant::now();
                        if let Some(state) = file_state.as_mut() {
                            if let Ok(line) = serde_json::to_string(&record) {
                                let _ = state.write_line(&line);
                            }
                        }

                        if cfg.console_table {
                            println!("{}", format_console_row(&record));
                        }

                        metrics_hook.increment_counter("logs_emitted_total", 1);
                        metrics_hook.observe_histogram(
                            "log_write_latency_ms",
                            write_start.elapsed().as_secs_f64() * 1000.0,
                        );
                        if let Some(duration_ms) = record.duration_ms {
                            metrics_hook.observe_histogram("event_duration_ms", duration_ms as f64);
                        }
                    }
                }
            }
        })
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    Ok(())
}

struct LogFileState {
    base_path: PathBuf,
    writer: BufWriter<File>,
    current_bytes: u64,
    rotation: RotationConfig,
}

impl LogFileState {
    fn new(path: &str, rotation: RotationConfig) -> std::io::Result<Self> {
        let base_path = PathBuf::from(path);
        if let Some(parent) = base_path.parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&base_path)?;
        let current_bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            base_path,
            writer: BufWriter::new(file),
            current_bytes,
            rotation,
        })
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        self.current_bytes = self.current_bytes.saturating_add(line.len() as u64 + 1);

        if self.current_bytes >= self.rotation.max_bytes {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;
        let suffix = now_epoch_ms();
        let rotated = PathBuf::from(format!("{}.{}", self.base_path.display(), suffix));
        fs::rename(&self.base_path, &rotated)?;

        if self.rotation.compress {
            let compressed = PathBuf::from(format!("{}.gz", rotated.display()));
            compress_file(&rotated, &compressed)?;
            let _ = fs::remove_file(&rotated);
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.base_path)?;
        self.writer = BufWriter::new(file);
        self.current_bytes = 0;

        self.prune_rotated_files();
        Ok(())
    }

    fn prune_rotated_files(&self) {
        if self.rotation.max_files == 0 {
            return;
        }
        let parent = match self.base_path.parent() {
            Some(parent) => parent,
            None => return,
        };
        let base = match self.base_path.file_name().and_then(|name| name.to_str()) {
            Some(base) => base,
            None => return,
        };
        let mut rotated_files: Vec<(PathBuf, std::time::SystemTime)> = match fs::read_dir(parent) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let path = entry.path();
                    let name = path.file_name()?.to_str()?;
                    if !name.starts_with(&format!("{}.", base)) {
                        return None;
                    }
                    let modified = entry
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    Some((path, modified))
                })
                .collect(),
            Err(_) => return,
        };

        rotated_files.sort_by_key(|(_, modified)| *modified);
        if rotated_files.len() <= self.rotation.max_files {
            return;
        }
        let to_remove = rotated_files.len() - self.rotation.max_files;
        for (path, _) in rotated_files.into_iter().take(to_remove) {
            let _ = fs::remove_file(path);
        }
    }
}

fn compress_file(input_path: &Path, output_path: &Path) -> std::io::Result<()> {
    let input = fs::read(input_path)?;
    let output = File::create(output_path)?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    encoder.write_all(&input)?;
    let _ = encoder.finish()?;
    Ok(())
}

fn print_console_header() {
    println!(
        "{}",
        [
            fit("TS_UTC", 24),
            fit("LVL", 5),
            fit("SERVICE", 12),
            fit("NODE", 10),
            fit("AGENT", 22),
            fit("TRACE", 16),
            fit("SPAN", 16),
            fit("PARENT", 16),
            fit("EVENT", 12),
            fit("DUR_MS", 7),
            fit("MESSAGE", 40),
            "CONTEXT".to_string(),
        ]
        .join(" ")
    );
}

fn format_console_row(record: &LogRecord) -> String {
    let context = serde_json::to_string(&record.context).unwrap_or_else(|_| "{}".to_string());
    [
        fit(&record.timestamp, 24),
        fit(&record.level, 5),
        fit(&record.service, 12),
        fit(&record.node_id, 10),
        fit(record.agent_id.as_deref().unwrap_or("-"), 22),
        fit(&record.trace_id, 16),
        fit(&record.span_id, 16),
        fit(record.parent_span_id.as_deref().unwrap_or("-"), 16),
        fit(&record.event_type, 12),
        fit(
            &record
                .duration_ms
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            7,
        ),
        fit(&record.message, 40),
        context,
    ]
    .join(" ")
}

fn fit(input: &str, width: usize) -> String {
    let count = input.chars().count();
    if count == width {
        return input.to_string();
    }
    if count < width {
        let mut out = String::with_capacity(width);
        out.push_str(input);
        out.push_str(&" ".repeat(width - count));
        return out;
    }

    if width <= 3 {
        return input.chars().take(width).collect();
    }
    let mut out: String = input.chars().take(width - 3).collect();
    out.push_str("...");
    out
}

fn now_rfc3339_millis() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".to_string())
}

fn now_epoch_ms() -> i64 {
    let now = SystemTime::now();
    match now.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}

fn sample(rate: f64, trace_id: Option<&str>) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }

    let mut hasher = DefaultHasher::new();
    trace_id.unwrap_or("default").hash(&mut hasher);
    let value = hasher.finish() as f64 / u64::MAX as f64;
    value <= rate
}

fn redact_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                if is_sensitive_key(&key) {
                    out.insert(key, Value::String("[REDACTED]".to_string()));
                } else {
                    out.insert(key, redact_json(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json).collect()),
        Value::String(raw) => {
            if looks_like_secret(&raw) {
                Value::String("[REDACTED]".to_string())
            } else {
                Value::String(raw)
            }
        }
        other => other,
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "token",
        "secret",
        "password",
        "authorization",
        "access_token",
        "refresh_token",
        "private_key",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn looks_like_secret(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.starts_with("sk-")
        || lower.contains("api_key=")
        || lower.contains("token=")
        || is_jwt(raw)
}

fn is_jwt(raw: &str) -> bool {
    let parts: Vec<&str> = raw.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

fn next_hex_id_64() -> String {
    format!("{:016x}", next_counter_mix() as u64)
}

fn next_hex_id_128() -> String {
    let a = next_counter_mix() as u128;
    let b = next_counter_mix() as u128;
    format!("{:032x}", (a << 64) | b)
}

fn next_counter_mix() -> u128 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    now_ns ^ (c << 17) ^ (pid << 9)
}
