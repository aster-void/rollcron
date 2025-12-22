use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

use crate::config::Job;
use crate::env;
use crate::git;

use super::backoff::{calculate_backoff, generate_jitter};

pub fn resolve_work_dir(sot_path: &PathBuf, job_id: &str, working_dir: &Option<String>) -> PathBuf {
    let job_dir = git::get_job_dir(sot_path, job_id);
    match working_dir {
        Some(dir) => {
            let work_path = job_dir.join(dir);
            // Canonicalize to resolve .. and symlinks, then verify path is within job_dir
            match (work_path.canonicalize(), job_dir.canonicalize()) {
                (Ok(resolved), Ok(base)) if resolved.starts_with(&base) => resolved,
                _ => {
                    eprintln!(
                        "[job:{}] Invalid working_dir '{}': path traversal or non-existent",
                        job_id, dir
                    );
                    job_dir
                }
            }
        }
        None => job_dir,
    }
}

pub async fn execute_job(job: &Job, work_dir: &PathBuf) {
    let tag = format!("[job:{}]", job.id);

    // Apply task jitter before first execution
    if let Some(jitter_max) = job.jitter {
        let jitter = generate_jitter(jitter_max);
        if jitter > Duration::ZERO {
            println!("{} Applying jitter: {:?}", tag, jitter);
            sleep(jitter).await;
        }
    }

    let max_attempts = job.retry.as_ref().map(|r| r.max + 1).unwrap_or(1);

    for attempt in 0..max_attempts {
        if attempt > 0 {
            if let Some(retry) = job.retry.as_ref() {
                let delay = calculate_backoff(retry, attempt - 1);
                println!("{} Retry {}/{} after {:?}", tag, attempt, max_attempts - 1, delay);
                sleep(delay).await;
            }
        }

        println!("{} Starting '{}'", tag, job.name);
        println!("{}   command: {}", tag, job.command);

        let result = run_command(job, work_dir).await;
        let success = handle_result(&tag, job, &result);

        if success {
            return;
        }

        if attempt + 1 < max_attempts {
            println!("{} Will retry...", tag);
        }
    }
}

async fn run_command(job: &Job, work_dir: &PathBuf) -> CommandResult {
    // Load .env file if it exists
    let env_vars = match env::load_env_file(work_dir) {
        Ok(vars) => vars,
        Err(e) => {
            return CommandResult::ExecError(format!("Failed to load .env file: {}", e));
        }
    };

    let mut cmd = Command::new("sh");
    cmd.args(["-c", &job.command])
        .current_dir(work_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    // Apply environment variables from .env file
    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return CommandResult::ExecError(e.to_string()),
    };

    let result = tokio::time::timeout(job.timeout, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => CommandResult::Completed(output),
        Ok(Err(e)) => CommandResult::ExecError(e.to_string()),
        Err(_) => CommandResult::Timeout,
    }
}

enum CommandResult {
    Completed(std::process::Output),
    ExecError(String),
    Timeout,
}

fn print_output_lines(tag: &str, output: &str, use_stderr: bool) {
    if output.trim().is_empty() {
        return;
    }
    for line in output.lines() {
        if use_stderr {
            eprintln!("{}   | {}", tag, line);
        } else {
            println!("{}   | {}", tag, line);
        }
    }
}

fn handle_result(tag: &str, job: &Job, result: &CommandResult) -> bool {
    match result {
        CommandResult::Completed(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if output.status.success() {
                println!("{} ✓ Completed", tag);
                print_output_lines(tag, &stdout, false);
                true
            } else {
                eprintln!("{} ✗ Failed (exit code: {:?})", tag, output.status.code());
                print_output_lines(tag, &stderr, true);
                print_output_lines(tag, &stdout, true);
                false
            }
        }
        CommandResult::ExecError(e) => {
            eprintln!("{} ✗ Failed to execute: {}", tag, e);
            false
        }
        CommandResult::Timeout => {
            eprintln!("{} ✗ Timeout after {:?}", tag, job.timeout);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Concurrency, RetryConfig};
    use cron::Schedule;
    use std::str::FromStr;
    use tempfile::tempdir;

    fn make_job(cmd: &str, timeout_secs: u64) -> Job {
        Job {
            id: "test".to_string(),
            name: "Test Job".to_string(),
            schedule: Schedule::from_str("* * * * * *").unwrap(),
            command: cmd.to_string(),
            timeout: Duration::from_secs(timeout_secs),
            concurrency: Concurrency::Skip,
            retry: None,
            working_dir: None,
            jitter: None,
            enabled: true,
            timezone: None,
        }
    }

    #[tokio::test]
    async fn execute_simple_job() {
        let job = make_job("echo test", 10);
        let dir = tempdir().unwrap();
        execute_job(&job, &dir.path().to_path_buf()).await;
    }

    #[tokio::test]
    async fn job_timeout() {
        let job = make_job("sleep 10", 1);
        let dir = tempdir().unwrap();
        execute_job(&job, &dir.path().to_path_buf()).await;
    }

    #[tokio::test]
    async fn job_retry_on_failure() {
        let mut job = make_job("exit 1", 10);
        job.retry = Some(RetryConfig {
            max: 2,
            delay: Duration::from_millis(10),
            jitter: None,
        });
        let dir = tempdir().unwrap();
        let start = std::time::Instant::now();
        execute_job(&job, &dir.path().to_path_buf()).await;
        assert!(start.elapsed() >= Duration::from_millis(30));
    }

    #[tokio::test]
    async fn job_success_no_retry() {
        let mut job = make_job("echo ok", 10);
        job.retry = Some(RetryConfig {
            max: 3,
            delay: Duration::from_millis(100),
            jitter: None,
        });
        let dir = tempdir().unwrap();
        let start = std::time::Instant::now();
        execute_job(&job, &dir.path().to_path_buf()).await;
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn job_with_task_jitter() {
        let mut job = make_job("echo ok", 10);
        job.jitter = Some(Duration::from_millis(50));
        let dir = tempdir().unwrap();
        let start = std::time::Instant::now();
        execute_job(&job, &dir.path().to_path_buf()).await;
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn job_with_env_file() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(&env_path, "TEST_VAR=hello\nOTHER_VAR=world").unwrap();

        let job = make_job("echo $TEST_VAR $OTHER_VAR", 10);
        execute_job(&job, &dir.path().to_path_buf()).await;
    }

    #[tokio::test]
    async fn job_without_env_file() {
        let dir = tempdir().unwrap();
        // No .env file created - should work fine
        let job = make_job("echo no env file", 10);
        execute_job(&job, &dir.path().to_path_buf()).await;
    }
}
