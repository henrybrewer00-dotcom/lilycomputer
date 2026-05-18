use anyhow::{Context, Result};
use rand::RngCore;
use std::path::{Path, PathBuf};

pub fn shared_dir() -> PathBuf {
    PathBuf::from("/Users/Shared/lily")
}

pub fn shared_token_path() -> PathBuf {
    shared_dir().join("token")
}

pub fn user_config_dir() -> PathBuf {
    dirs_home().join(".lily")
}

pub fn user_env_path() -> PathBuf {
    user_config_dir().join("env")
}

pub fn shared_env_path() -> PathBuf {
    shared_dir().join("env")
}

pub fn user_log_dir() -> PathBuf {
    dirs_home().join("Library/Logs/lily")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Read the shared token; if missing and `create_if_missing`, generate a fresh one.
pub fn load_or_create_token(create_if_missing: bool) -> Result<String> {
    let path = shared_token_path();
    if path.exists() {
        return std::fs::read_to_string(&path)
            .map(|s| s.trim().to_owned())
            .with_context(|| format!("read token at {}", path.display()));
    }
    if !create_if_missing {
        anyhow::bail!(
            "token not found at {} — is lilyd installed on the worker user?",
            path.display()
        );
    }
    let dir = shared_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    // Ensure directory is world-traversable so the other user can read the token.
    set_mode(&dir, 0o755).ok();

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(&path, &token).with_context(|| format!("write token to {}", path.display()))?;
    set_mode(&path, 0o644).ok();
    Ok(token)
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)?.permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(p, perms)
}

#[cfg(not(unix))]
fn set_mode(_p: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// Read GROQ_API_KEY from environment, falling back to ~/.lily/env or
/// /Users/Shared/lily/env (KEY=VALUE per line). The shared location is useful
/// when the same human runs both accounts and wants a single source of truth.
pub fn load_groq_key() -> Result<String> {
    if let Ok(k) = std::env::var("GROQ_API_KEY") {
        if !k.is_empty() {
            return Ok(k);
        }
    }
    for path in [user_env_path(), shared_env_path()] {
        if let Some(k) = read_key_from_envfile(&path, "GROQ_API_KEY")? {
            if !k.is_empty() {
                return Ok(k);
            }
        }
    }
    anyhow::bail!(
        "GROQ_API_KEY not found. Set in env, in {}, or in {}",
        user_env_path().display(),
        shared_env_path().display(),
    )
}

fn read_key_from_envfile(path: &Path, key: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Ok(Some(v.trim().trim_matches('"').to_owned()));
            }
        }
    }
    Ok(None)
}
