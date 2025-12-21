use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Ensures repo is cloned/synced to cache. Returns cache path.
pub fn ensure_repo(source: &str) -> Result<PathBuf> {
    let cache_dir = get_cache_dir(source)?;
    if let Some(parent) = cache_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if cache_dir.exists() {
        sync_repo(&cache_dir)?;
    } else {
        clone_repo(source, &cache_dir)?;
    }

    Ok(cache_dir)
}

fn clone_repo(source: &str, dest: &Path) -> Result<()> {
    let dest_str = dest
        .to_str()
        .context("Destination path contains invalid UTF-8")?;
    let output = Command::new("git")
        .args(["clone", source, dest_str])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr);
    }

    Ok(())
}

fn sync_repo(dest: &Path) -> Result<()> {
    // git clone sets up tracking branches for both local and remote repos
    let has_upstream = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(dest)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_upstream {
        let output = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(dest)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git pull failed: {}", stderr);
        }
    }

    Ok(())
}

fn get_cache_dir(source: &str) -> Result<PathBuf> {
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rollcron");

    let repo_name = source
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/')
        .next()
        .unwrap_or("repo");

    let hash = &format!("{:x}", hash_str(source))[..8];

    Ok(cache_base.join(format!("{}-{}", repo_name, hash)))
}

fn hash_str(input: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

pub fn get_job_dir(sot_path: &Path, job_id: &str) -> PathBuf {
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rollcron");

    let sot_name = sot_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    cache_base.join(format!("{}@{}", sot_name, job_id))
}

pub fn sync_to_job_dir(sot_path: &Path, job_dir: &Path) -> Result<()> {
    let job_dir_str = job_dir
        .to_str()
        .context("Job directory path contains invalid UTF-8")?;

    // Use atomic temp directory to avoid TOCTOU race condition
    let temp_dir = job_dir.with_extension("tmp");
    let temp_dir_str = temp_dir
        .to_str()
        .context("Temp directory path contains invalid UTF-8")?;

    // Clean up any leftover temp directory
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir)?;
    }
    std::fs::create_dir_all(&temp_dir)?;

    // Use git archive for all repos (both local and remote use git clone now)
    let archive = Command::new("git")
        .args(["archive", "HEAD"])
        .current_dir(sot_path)
        .output()?;

    if !archive.status.success() {
        std::fs::remove_dir_all(&temp_dir)?;
        let stderr = String::from_utf8_lossy(&archive.stderr);
        anyhow::bail!("git archive failed: {}", stderr);
    }

    // Extract with security flags to prevent path traversal
    let mut extract = Command::new("tar")
        .args(["--no-absolute-file-names", "-x"])
        .current_dir(&temp_dir)
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    {
        use std::io::Write;
        let stdin = extract
            .stdin
            .as_mut()
            .context("Failed to open tar stdin")?;
        stdin.write_all(&archive.stdout)?;
    }

    let status = extract.wait()?;
    if !status.success() {
        std::fs::remove_dir_all(&temp_dir)?;
        anyhow::bail!("tar extraction failed with exit code: {:?}", status.code());
    }

    // Atomic swap: remove old, rename temp to target
    if job_dir.exists() {
        std::fs::remove_dir_all(job_dir)?;
    }
    std::fs::rename(&temp_dir, job_dir).with_context(|| {
        format!("Failed to rename {} to {}", temp_dir_str, job_dir_str)
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_from_url() {
        let dir = get_cache_dir("https://github.com/user/myrepo.git").unwrap();
        assert!(dir.to_str().unwrap().contains("myrepo"));
    }
}
