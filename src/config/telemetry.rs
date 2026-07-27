//! `config/telemetry.php` — OpenTelemetry.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{
    TELEMETRY_ENABLED, TELEMETRY_ENDPOINT, TELEMETRY_SAMPLE_RATIO, TELEMETRY_SERVICE_NAME,
};

/// Telemetry settings, read by `bootstrap.rs`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Cheaper than it sounds: with no endpoint this is a header read, a header
    // written and a trace id on every log line — no collector, no exporter, no
    // dependency. Worth turning on well before you have somewhere to send
    // spans.
    config.set(TELEMETRY_ENABLED, env.bool("TELEMETRY_ENABLED", false))?;

    // Absent means propagate but do not export. Setting it needs the `otlp`
    // cargo feature; without it, `bootstrap.rs` says so rather than exporting
    // nothing in silence.
    if let Some(endpoint) = env.get("OTEL_EXPORTER_OTLP_ENDPOINT") {
        config.set(TELEMETRY_ENDPOINT, endpoint)?;
    }

    config.set(TELEMETRY_SERVICE_NAME, env.string("OTEL_SERVICE_NAME", "rainier-sample"))?;

    // Applies only to traces this service *starts*. One arriving with a
    // decision keeps it — a trace sampled in half its services has holes, and
    // a hole is indistinguishable from a call that never happened.
    config.set(TELEMETRY_SAMPLE_RATIO, env.float("OTEL_TRACES_SAMPLER_ARG", 1.0))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_unless_asked_for() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(TELEMETRY_ENABLED), Some(false));
        assert_eq!(config.get(TELEMETRY_ENDPOINT), None);
    }

    #[test]
    fn the_standard_environment_variables_are_the_ones_read() {
        // `OTEL_*` is what every other OpenTelemetry SDK reads, so a
        // deployment that already sets them needs no Rainier-specific ones.
        let config = Config::new();
        configure(
            &config,
            &Env::parse(
                "TELEMETRY_ENABLED=true
OTEL_EXPORTER_OTLP_ENDPOINT=http://collector:4317
OTEL_SERVICE_NAME=posts-api
OTEL_TRACES_SAMPLER_ARG=0.1",
            ),
        )
        .unwrap();

        assert_eq!(config.get(TELEMETRY_ENABLED), Some(true));
        assert_eq!(config.get(TELEMETRY_ENDPOINT).as_deref(), Some("http://collector:4317"));
        assert_eq!(config.get(TELEMETRY_SERVICE_NAME).as_deref(), Some("posts-api"));
        assert_eq!(config.get(TELEMETRY_SAMPLE_RATIO), Some(0.1));
    }

    #[test]
    fn sampling_everything_is_the_default() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(TELEMETRY_SAMPLE_RATIO), Some(1.0));
    }
}
