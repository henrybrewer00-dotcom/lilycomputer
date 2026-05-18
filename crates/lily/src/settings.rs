use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub intro_animation: bool,
    pub theme: Theme,
    pub auto_view_screenshots: bool,
    pub persistent_memory: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            intro_animation: true,
            theme: Theme::Dark,
            auto_view_screenshots: false,
            persistent_memory: true,
        }
    }
}

fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".lily").join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&p, s);
        }
    }
}

/// Settings entries surfaced in the /settings UI, in display order.
#[derive(Debug, Clone, Copy)]
pub enum Field {
    IntroAnimation,
    Theme,
    AutoViewScreenshots,
    PersistentMemory,
}

impl Field {
    pub fn all() -> &'static [Field] {
        &[
            Field::IntroAnimation,
            Field::Theme,
            Field::AutoViewScreenshots,
            Field::PersistentMemory,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Field::IntroAnimation => "Intro animation",
            Field::Theme => "Theme",
            Field::AutoViewScreenshots => "Auto-open screenshots in Preview",
            Field::PersistentMemory => "Persistent memory across sessions",
        }
    }

    pub fn value_text(&self, s: &Settings) -> String {
        match self {
            Field::IntroAnimation => if s.intro_animation { "on".into() } else { "off".into() },
            Field::Theme => match s.theme { Theme::Dark => "dark".into(), Theme::Light => "light".into() },
            Field::AutoViewScreenshots => if s.auto_view_screenshots { "on".into() } else { "off".into() },
            Field::PersistentMemory => if s.persistent_memory { "on".into() } else { "off".into() },
        }
    }

    pub fn toggle(&self, s: &mut Settings) {
        match self {
            Field::IntroAnimation => s.intro_animation = !s.intro_animation,
            Field::Theme => s.theme = match s.theme { Theme::Dark => Theme::Light, Theme::Light => Theme::Dark },
            Field::AutoViewScreenshots => s.auto_view_screenshots = !s.auto_view_screenshots,
            Field::PersistentMemory => s.persistent_memory = !s.persistent_memory,
        }
    }
}
