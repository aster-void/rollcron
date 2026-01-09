mod config;
mod env;
mod git;
mod logging;
mod scheduler;

use anyhow::{Context, Result};
use clap::Parser;
use scheduler::{ConfigUpdate, Scheduler, SyncRequest};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};
use xtra::prelude::*;

const CONFIG_FILE: &str = "rollcron.yaml";

#[derive(Parser)]
#[command(name = "rollcron", about = "Auto-pulling cron scheduler")]
struct Args {
    /// Path to local repo or remote URL (https://... or git@...)
    repo: String,

    /// Pull interval in seconds
    #[arg(long, default_value = "3600")]
    pull_interval: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    logging::init();
    let args = Args::parse();

    // Expand shell variables (~, $VAR) and canonicalize local paths
    let expanded_repo = env::expand_string(&args.repo);
    let source = if expanded_repo.starts_with('/') || expanded_repo.starts_with('.') {
        PathBuf::from(&expanded_repo)
            .canonicalize()?
            .to_str()
            .context("Path contains invalid UTF-8")?
            .to_string()
    } else {
        expanded_repo
    };

    info!(source = %source, pull_interval = args.pull_interval, "Starting rollcron");

    // Initial sync
    let (sot_path, _) = git::ensure_repo(&source)?;
    info!(cache = %sot_path.display(), "Repository ready");

    let (initial_runner, initial_jobs) = load_config(&sot_path)?;

    // Initial sync of all job directories
    for job in &initial_jobs {
        let job_dir = git::get_job_dir(&sot_path, &job.id);
        git::sync_to_job_dir(&sot_path, &job_dir)?;
    }

    // Spawn Scheduler actor
    let scheduler = xtra::spawn_tokio(
        Scheduler::new(initial_jobs, sot_path.clone(), initial_runner),
        Mailbox::unbounded(),
    );

    // Spawn auto-sync task
    let source_clone = source.clone();
    let pull_interval = args.pull_interval;
    let scheduler_clone = scheduler.clone();
    let sync_handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(pull_interval));
        loop {
            ticker.tick().await;

            let (sot, update_info) = match git::ensure_repo(&source_clone) {
                Ok(r) => r,
                Err(e) => {
                    error!(error = %e, "Sync failed");
                    continue;
                }
            };

            let Some(range) = update_info else {
                continue;
            };

            info!(range = %range, "Pulled updates");

            match load_config(&sot) {
                Ok((runner, jobs)) => {
                    // Mark all jobs as needing sync
                    let job_ids: Vec<String> = jobs.iter().map(|j| j.id.clone()).collect();
                    if let Err(e) = scheduler_clone
                        .send(SyncRequest {
                            job_ids,
                            sot_path: sot,
                        })
                        .await
                    {
                        error!(error = %e, "Failed to queue sync");
                        continue;
                    }
                    // Update config
                    if let Err(e) = scheduler_clone.send(ConfigUpdate { jobs, runner }).await {
                        error!(error = %e, "Failed to update config");
                    }
                }
                Err(e) => error!(error = %e, "Failed to reload config"),
            }
        }
    });

    // Wait for shutdown signal or task panic
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
        }
        result = sync_handle => {
            match result {
                Ok(()) => warn!("Sync task exited unexpectedly"),
                Err(e) => error!(error = %e, "Sync task panicked"),
            }
        }
    }

    Ok(())
}

fn load_config(sot_path: &PathBuf) -> Result<(config::RunnerConfig, Vec<config::Job>)> {
    let config_path = sot_path.join(CONFIG_FILE);
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", config_path.display(), e))?;
    config::parse_config(&content)
}
