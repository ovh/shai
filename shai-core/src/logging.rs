use chrono;
use std::fmt;
use std::path::PathBuf;
use tracing::{Event, Subscriber};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::format::FmtSpan,
    fmt::{format::Writer, FormatEvent, FormatFields},
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Custom formatter that colors different event types
struct ColoredFormatter;

impl<S, N> FormatEvent<S, N> for ColoredFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let metadata = event.metadata();

        // Target colors (different from log level colors)
        let (target_color, message_color) = if metadata.target() == "agent::command" {
            ("\x1b[38;5;213m", "") // Bright pink for commands, normal message
        } else if metadata.target() == "agent::internal_event" {
            ("\x1b[38;5;51m", "") // Bright cyan for internal events, normal message
        } else if metadata.target() == "agent::public_event" {
            ("\x1b[38;5;226m", "") // Bright yellow for public events, normal message
        } else if metadata.target() == "agent::status" {
            ("\x1b[38;5;82m", "") // Bright lime green for status changes, normal message
        } else if metadata.target() == "misc" {
            ("\x1b[38;5;208m", "") // Bright orange for misc debugging, normal message
        } else if metadata.target() == "brain::coder" {
            ("\x1b[38;5;128m", "")
        } else {
            ("\x1b[2m", "\x1b[2m") // Dim for both target and message for other logs
        };

        // Level colors (standard tracing colors)
        let level_color = match *metadata.level() {
            tracing::Level::ERROR => "\x1b[31m", // Red
            tracing::Level::WARN => "\x1b[33m",  // Yellow
            tracing::Level::INFO => "\x1b[32m",  // Green
            tracing::Level::DEBUG => "\x1b[34m", // Blue
            tracing::Level::TRACE => "\x1b[35m", // Purple
        };

        // Format: [timestamp] [colored_level] [colored_target] colored_or_dim_message
        write!(
            writer,
            "{} ",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ")
        )?;
        write!(writer, "{}{:5}\x1b[0m ", level_color, metadata.level())?;
        write!(writer, "{}[{}]\x1b[0m ", target_color, metadata.target())?;
        write!(writer, "{}", message_color)?;
        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer, "\x1b[0m")?;

        Ok(())
    }
}

/// Logging configuration for the agent
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Log level filter (e.g., "debug", "info", "warn", "error")
    pub level: String,
    /// Optional file path for log output. If None, logs to stdout
    pub file_path: Option<PathBuf>,
    /// Whether to include spans in logs (for debugging performance)
    pub include_spans: bool,
    /// JSON format instead of human-readable
    pub json_format: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "off".to_string(),
            file_path: None,
            include_spans: false,
            json_format: false,
        }
    }
}

impl LoggingConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self {
            level: std::env::var("SHAI_LOG_LEVEL").unwrap_or_else(|_| "off".to_string()),
            file_path: std::env::var("SHAI_LOG_FILE").ok().map(PathBuf::from),
            include_spans: std::env::var("SHAI_LOG_SPANS")
                .map(|v| v == "true")
                .unwrap_or(false),
            json_format: std::env::var("SHAI_LOG_JSON")
                .map(|v| v == "true")
                .unwrap_or(false),
        }
    }

    /// Set log level
    pub fn level<S: Into<String>>(mut self, level: S) -> Self {
        self.level = level.into();
        self
    }

    /// Set log file path
    pub fn file_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Enable span logging
    pub fn with_spans(mut self, enable: bool) -> Self {
        self.include_spans = enable;
        self
    }

    /// Enable JSON format
    pub fn json_format(mut self, enable: bool) -> Self {
        self.json_format = enable;
        self
    }

    /// Initialize the global tracing subscriber (safe for multiple calls)
    pub fn init(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Set default level for all modules, then override specific shai modules
        let filter = EnvFilter::from_default_env()
            .add_directive("warn".parse()?)
            .add_directive(format!("shai_core={}", self.level).parse()?)
            .add_directive(format!("brain::coder={}", self.level).parse()?)
            .add_directive(format!("brain::searcher={}", self.level).parse()?)
            .add_directive(format!("agent::command={}", self.level).parse()?)
            .add_directive(format!("agent::tool_completed={}", self.level).parse()?)
            .add_directive(format!("agent::internal_event={}", self.level).parse()?)
            .add_directive(format!("agent::public_event={}", self.level).parse()?)
            .add_directive(format!("agent::status={}", self.level).parse()?)
            .add_directive(format!("agent::loop={}", self.level).parse()?)
            .add_directive(format!("misc={}", self.level).parse()?);

        let span_events = if self.include_spans {
            FmtSpan::NEW | FmtSpan::CLOSE
        } else {
            FmtSpan::NONE
        };

        match self.file_path {
            Some(path) => {
                let file_appender = RollingFileAppender::new(
                    Rotation::DAILY,
                    path.parent().unwrap_or_else(|| std::path::Path::new(".")),
                    path.file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("agent.log")),
                );

                if self.json_format {
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .json()
                                .with_writer(file_appender)
                                .with_span_events(span_events),
                        )
                        .try_init()
                        .map_err(|_| "Failed to initialize subscriber (already set)")?;
                } else {
                    // File output without colors (colors don't work well in files)
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .with_writer(file_appender)
                                .with_span_events(span_events)
                                .with_ansi(false),
                        )
                        .try_init()
                        .map_err(|_| "Failed to initialize subscriber (already set)")?;
                }
            }
            None => {
                if self.json_format {
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .json()
                                .with_span_events(span_events),
                        )
                        .try_init()
                        .map_err(|_| "Failed to initialize subscriber (already set)")?;
                } else {
                    // Console output with custom colors
                    tracing_subscriber::registry()
                        .with(filter)
                        .with(
                            tracing_subscriber::fmt::layer()
                                .event_format(ColoredFormatter)
                                .with_ansi(true),
                        )
                        .try_init()
                        .map_err(|_| "Failed to initialize subscriber (already set)")?;
                }
            }
        }

        Ok(())
    }
}
