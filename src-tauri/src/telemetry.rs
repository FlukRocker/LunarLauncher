//! OpenTelemetry wiring for the launcher and the game it spawns.
//!
//! Deliberately opt-in with no default endpoint. A launcher knows the player's
//! username, which servers they join and when they play; that is not data to
//! ship anywhere by default. Telemetry activates only when the user supplies
//! a collector they control, and the settings copy says what leaves the
//! machine.

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Persisted telemetry settings. Part of config.json, so it round-trips with
/// everything else.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryConfig {
    /// Off unless the user turns it on.
    #[serde(default)]
    pub enabled: bool,
    /// OTLP/HTTP collector, e.g. http://localhost:4318. No default: without
    /// one there is nowhere to send, which is the point.
    #[serde(default)]
    pub endpoint: String,
    /// Also instrument the game via the OpenTelemetry Java agent, when the
    /// jar is present at this path.
    #[serde(default)]
    pub java_agent_path: Option<std::path::PathBuf>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            java_agent_path: None,
        }
    }
}

impl TelemetryConfig {
    /// Usable only when switched on *and* given somewhere to send.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.endpoint.trim().is_empty()
    }
}

/// Build the OTLP tracer and return a `tracing` layer for it.
///
/// Returns Ok(None) when telemetry is inactive, so the caller installs a
/// plain subscriber rather than treating "off" as an error.
pub fn build_layer<S>(
    config: &TelemetryConfig,
) -> Result<Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{trace, Resource};

    if !config.is_active() {
        return Ok(None);
    }

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/traces", config.endpoint.trim_end_matches('/')))
        .build()
        .map_err(|e| crate::error::Error::Other(format!("OTLP exporter: {e}")))?;

    let provider = trace::TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(Resource::new(vec![
            KeyValue::new("service.name", "lunar-launcher"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("os.type", std::env::consts::OS),
            KeyValue::new("host.arch", std::env::consts::ARCH),
        ]))
        .build();

    let tracer = provider.tracer("lunar-launcher");
    opentelemetry::global::set_tracer_provider(provider);

    Ok(Some(tracing_opentelemetry::layer().with_tracer(tracer)))
}

/// Flush pending spans. Called on shutdown so a short session still reports.
pub fn shutdown() {
    opentelemetry::global::shutdown_tracer_provider();
}

/// JVM arguments that attach the OpenTelemetry Java agent to the game.
///
/// Returns nothing unless telemetry is active and the agent jar actually
/// exists — a missing `-javaagent` path makes the JVM refuse to start, so a
/// stale setting must not be able to block launching.
pub fn java_agent_args(config: &TelemetryConfig, server_id: &str) -> Vec<String> {
    if !config.is_active() {
        return Vec::new();
    }
    let Some(path) = &config.java_agent_path else {
        return Vec::new();
    };
    if !path.exists() {
        tracing::warn!(
            path = %path.display(),
            "OpenTelemetry Java agent not found; launching without it"
        );
        return Vec::new();
    }

    vec![
        format!("-javaagent:{}", path.display()),
        "-Dotel.traces.exporter=otlp".into(),
        "-Dotel.metrics.exporter=otlp".into(),
        "-Dotel.logs.exporter=none".into(),
        format!("-Dotel.exporter.otlp.protocol=http/protobuf"),
        format!(
            "-Dotel.exporter.otlp.endpoint={}",
            config.endpoint.trim_end_matches('/')
        ),
        "-Dotel.service.name=minecraft".into(),
        format!("-Dotel.resource.attributes=lunar.server={server_id}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_by_default() {
        let c = TelemetryConfig::default();
        assert!(!c.enabled);
        assert!(c.endpoint.is_empty());
        assert!(!c.is_active());
    }

    #[test]
    fn enabling_without_an_endpoint_stays_inactive() {
        let c = TelemetryConfig { enabled: true, endpoint: "  ".into(), java_agent_path: None };
        assert!(!c.is_active(), "nowhere to send means nothing is sent");
    }

    #[test]
    fn active_only_when_both_are_set() {
        let c = TelemetryConfig {
            enabled: true,
            endpoint: "http://localhost:4318".into(),
            java_agent_path: None,
        };
        assert!(c.is_active());
    }

    #[test]
    fn no_agent_args_when_inactive() {
        let c = TelemetryConfig {
            enabled: false,
            endpoint: "http://localhost:4318".into(),
            java_agent_path: Some("/tmp/agent.jar".into()),
        };
        assert!(java_agent_args(&c, "Srv").is_empty());
    }

    #[test]
    fn missing_agent_jar_is_skipped_rather_than_breaking_launch() {
        let c = TelemetryConfig {
            enabled: true,
            endpoint: "http://localhost:4318".into(),
            java_agent_path: Some("/nonexistent/agent.jar".into()),
        };
        assert!(
            java_agent_args(&c, "Srv").is_empty(),
            "a stale path must not stop the game starting"
        );
    }

    #[test]
    fn agent_args_carry_endpoint_and_server() {
        let jar = std::env::temp_dir().join("lunar-otel-agent-test.jar");
        std::fs::write(&jar, b"x").unwrap();
        let c = TelemetryConfig {
            enabled: true,
            endpoint: "http://localhost:4318/".into(),
            java_agent_path: Some(jar.clone()),
        };
        let args = java_agent_args(&c, "Lunar_1.20.1");
        assert!(args[0].starts_with("-javaagent:"));
        assert!(args.iter().any(|a| a == "-Dotel.exporter.otlp.endpoint=http://localhost:4318"),
            "trailing slash must be trimmed: {args:?}");
        assert!(args.iter().any(|a| a.contains("lunar.server=Lunar_1.20.1")));
        assert!(args.iter().any(|a| a == "-Dotel.service.name=minecraft"));
        let _ = std::fs::remove_file(&jar);
    }
}
