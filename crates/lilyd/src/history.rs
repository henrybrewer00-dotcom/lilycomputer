use serde_json::Value;
use std::path::PathBuf;

/// Cap on persisted history. Keeps the system prompt + last N transcript messages.
const MAX_PERSISTED: usize = 80;

#[derive(Debug, Default)]
pub struct History {
    pub messages: Vec<Value>,
}

impl History {
    pub fn load() -> Self {
        let p = path();
        let Ok(s) = std::fs::read_to_string(&p) else { return Self::default(); };
        match serde_json::from_str::<Vec<Value>>(&s) {
            Ok(messages) => Self { messages },
            Err(e) => {
                tracing::warn!("history: ignoring corrupt file {}: {e}", p.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = p.with_extension("json.tmp");
        match serde_json::to_vec(&self.messages) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&tmp, &bytes) {
                    tracing::warn!("history save (tmp): {e}");
                    return;
                }
                if let Err(e) = std::fs::rename(&tmp, &p) {
                    tracing::warn!("history save (rename): {e}");
                }
            }
            Err(e) => tracing::warn!("history serialize: {e}"),
        }
    }

    pub fn reset(&mut self) {
        self.messages.clear();
        self.save();
    }
}

fn path() -> PathBuf {
    lily_core::config::user_config_dir().join("history.json")
}

/// Drop synthetic image follow-up messages (huge base64 blobs) so we keep
/// persisted history compact and re-prompt cost low. The model can always
/// take a fresh screenshot on the next turn.
pub fn strip_images(messages: Vec<Value>) -> Vec<Value> {
    messages
        .into_iter()
        .filter(|m| {
            let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role != "user" {
                return true;
            }
            let Some(arr) = m.get("content").and_then(|c| c.as_array()) else {
                return true;
            };
            !arr.iter().any(|b| {
                b.get("type").and_then(|t| t.as_str()) == Some("image_url")
            })
        })
        .collect()
}

/// Trim to the last MAX_PERSISTED messages, preserving the system prompt.
/// Truncates only at user-message boundaries so tool_call ↔ tool_result pairs stay intact.
pub fn cap(messages: Vec<Value>) -> Vec<Value> {
    if messages.len() <= MAX_PERSISTED {
        return messages;
    }
    let system = messages
        .first()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .cloned();

    // Find user-message indices.
    let user_idxs: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .map(|(i, _)| i)
        .collect();
    if user_idxs.is_empty() {
        return messages; // nothing structured to cap on
    }
    // Walk back from the end; find the earliest user-msg cutoff that keeps len ≤ MAX_PERSISTED.
    let mut cutoff = 0;
    for &idx in user_idxs.iter() {
        let remaining = messages.len() - idx;
        if remaining + system.is_some() as usize <= MAX_PERSISTED {
            cutoff = idx;
            break;
        }
    }
    let mut out: Vec<Value> = Vec::new();
    if let Some(s) = system {
        out.push(s);
    }
    out.extend(messages.into_iter().skip(cutoff));
    out
}
