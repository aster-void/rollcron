use tracing_subscriber::{fmt, EnvFilter};

/// Initialize the logging system with per-service filtering.
///
/// Log levels can be controlled via RUST_LOG environment variable:
/// - RUST_LOG=info                    - All services at info level
/// - RUST_LOG=rollcron=debug          - rollcron at debug, others at default
/// - RUST_LOG=rollcron::scheduler=trace,rollcron::git=warn
///
/// Available targets:
/// - rollcron           - main application
/// - rollcron::scheduler - job scheduling
/// - rollcron::git      - git operations
/// - rollcron::webhook  - webhook notifications
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .without_time()
        .init();
}
