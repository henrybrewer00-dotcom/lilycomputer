use crate::client::DaemonClient;
use crate::ui;
use anyhow::Result;
use crossterm::event::{Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::stream::Stream;
use futures_util::StreamExt;
use lily_core::protocol::Event as LilyEvent;
use ratatui::backend::Backend;
use ratatui::Terminal;
use std::pin::Pin;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Line {
    Banner { text: String },
    Prompt { text: String },
    PendingTool { id: String, label: String, frame: usize },
    DoneTool { label: String, elapsed_ms: u64, ok: bool },
    Assistant { text: String },
    DoneRun { summary: String },
    Screenshot { path: String, index: u32 },
    Info { text: String },
    Error { message: String },
}

pub struct AppState {
    pub client: DaemonClient,
    pub input: String,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub lines: Vec<Line>,
    pub session_id: Option<String>,
    pub running: bool,
    pub tokens: (u32, u32, u32),
    pub tool_count: u32,
    pub run_started: Option<Instant>,
    pub last_status: String,
    pub spinner_frame: usize,
    pub latest_shot: Option<(String, u32)>,
    pub auto_view: bool,
    pub last_auto_viewed: Option<u32>,
    pub settings: crate::settings::Settings,
    pub settings_open: bool,
    pub settings_cursor: usize,
    pub intro_started: Option<Instant>,
    pub intro_lines_shown: usize,
    pub quit: bool,
}

impl AppState {
    pub fn new(client: DaemonClient) -> Self {
        let settings = crate::settings::Settings::load();
        // If intro animation is on, start with empty lines — they reveal one
        // per tick. Otherwise pre-populate with the full banner.
        let mut lines: Vec<Line> = Vec::new();
        let intro_started = if settings.intro_animation {
            Some(Instant::now())
        } else {
            for l in crate::BANNER.lines() {
                lines.push(Line::Banner { text: l.to_string() });
            }
            lines.push(Line::Banner { text: String::new() });
            lines.push(Line::Info {
                text: "ask from terminal — Lily drives Chrome and macOS apps. /help · /settings · /clear".into(),
            });
            lines.push(Line::Info { text: "".into() });
            None
        };
        Self {
            client,
            input: String::new(),
            history: Vec::new(),
            history_idx: None,
            lines,
            session_id: None,
            running: false,
            tokens: (0, 0, 0),
            tool_count: 0,
            run_started: None,
            last_status: "idle".into(),
            spinner_frame: 0,
            latest_shot: None,
            auto_view: settings.auto_view_screenshots,
            last_auto_viewed: None,
            settings,
            settings_open: false,
            settings_cursor: 0,
            intro_started,
            intro_lines_shown: 0,
            quit: false,
        }
    }

    /// Drive the intro animation: reveal one banner line every 60ms until done.
    pub fn tick_intro(&mut self) {
        let Some(started) = self.intro_started else { return; };
        let banner_lines: Vec<&str> = crate::BANNER.lines().collect();
        let total = banner_lines.len() + 2; // banner + 2 info lines
        let elapsed_ms = started.elapsed().as_millis();
        let target = ((elapsed_ms / 60) as usize).min(total);
        while self.intro_lines_shown < target {
            let idx = self.intro_lines_shown;
            if idx < banner_lines.len() {
                self.lines.push(Line::Banner { text: banner_lines[idx].to_string() });
            } else if idx == banner_lines.len() {
                self.lines.push(Line::Banner { text: String::new() });
            } else {
                self.lines.push(Line::Info {
                    text: "ask from terminal — Lily drives Chrome and macOS apps. /help · /settings · /clear".into(),
                });
            }
            self.intro_lines_shown += 1;
        }
        if self.intro_lines_shown >= total {
            self.intro_started = None; // done
        }
    }
}

fn open_in_preview(path: &str) {
    let _ = std::process::Command::new("/usr/bin/open")
        .args(["-a", "Preview", path])
        .spawn();
}

pub async fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
    events: &mut EventStream,
) -> Result<()> {
    // Open the SSE stream from the daemon.
    let mut sse: Pin<Box<dyn Stream<Item = Result<LilyEvent>> + Send>> = state.client.stream().await?;
    let mut tick = tokio::time::interval(Duration::from_millis(80));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.draw(|f| ui::draw(f, state))?;
        if state.quit { break; }

        tokio::select! {
            biased;
            maybe_term = events.next() => {
                match maybe_term {
                    Some(Ok(ev)) => handle_terminal_event(state, ev).await?,
                    Some(Err(e)) => { state.lines.push(Line::Error { message: format!("term: {e}") }); }
                    None => break,
                }
            }
            maybe_sse = sse.next() => {
                match maybe_sse {
                    Some(Ok(ev)) => handle_lily_event(state, ev),
                    Some(Err(e)) => {
                        state.lines.push(Line::Error { message: format!("sse: {e}") });
                        // Re-connect.
                        if let Ok(s) = state.client.stream().await { sse = s; }
                    }
                    None => {
                        state.lines.push(Line::Error { message: "sse closed; reconnecting".into() });
                        if let Ok(s) = state.client.stream().await { sse = s; }
                    }
                }
            }
            _ = tick.tick() => {
                state.spinner_frame = state.spinner_frame.wrapping_add(1);
                for ln in state.lines.iter_mut() {
                    if let Line::PendingTool { frame, .. } = ln { *frame = frame.wrapping_add(1); }
                }
                state.tick_intro();
            }
        }
    }
    Ok(())
}

async fn handle_terminal_event(state: &mut AppState, ev: CtEvent) -> Result<()> {
    let CtEvent::Key(k) = ev else { return Ok(()); };
    if k.kind != KeyEventKind::Press { return Ok(()); }
    let KeyEvent { code, modifiers, .. } = k;

    // Settings overlay captures keys before normal handling.
    if state.settings_open {
        use crate::settings::Field;
        let fields = Field::all();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                state.settings_open = false;
                state.lines.push(Line::Info { text: "settings saved".into() });
            }
            KeyCode::Up => {
                if state.settings_cursor > 0 { state.settings_cursor -= 1; }
            }
            KeyCode::Down => {
                if state.settings_cursor + 1 < fields.len() { state.settings_cursor += 1; }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                fields[state.settings_cursor].toggle(&mut state.settings);
                state.auto_view = state.settings.auto_view_screenshots;
                state.settings.save();
            }
            _ => {}
        }
        return Ok(());
    }

    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    match (code, ctrl) {
        (KeyCode::Char('c'), true) => {
            if state.running {
                if let Some(id) = state.session_id.clone() {
                    let _ = state.client.cancel(id).await;
                }
            } else {
                state.quit = true;
            }
        }
        (KeyCode::Char('d'), true) => { state.quit = true; }
        (KeyCode::Char('r'), true) | (KeyCode::Char('l'), true) => {
            state.lines.clear();
            state.tokens = (0, 0, 0);
            state.tool_count = 0;
            state.last_status = "idle".into();
            let _ = state.client.reset().await;
            state.lines.push(Line::Assistant { text: "memory cleared".into() });
        }
        (KeyCode::Enter, _) => {
            let text = state.input.trim().to_string();
            if text.is_empty() { return Ok(()); }
            state.input.clear();
            state.history.push(text.clone());
            state.history_idx = None;

            // Slash commands handled locally.
            if let Some(cmd) = text.strip_prefix('/') {
                handle_slash(state, cmd).await;
                return Ok(());
            }

            submit_prompt(state, text).await;
        }
        (KeyCode::Backspace, _) => { state.input.pop(); }
        (KeyCode::Char('v'), false) if state.input.is_empty() => {
            if let Some((path, _)) = state.latest_shot.clone() { open_in_preview(&path); }
        }
        (KeyCode::Char(c), _) => { state.input.push(c); }
        (KeyCode::Up, _) => {
            if !state.history.is_empty() {
                let next = match state.history_idx {
                    None => state.history.len() - 1,
                    Some(0) => 0,
                    Some(i) => i - 1,
                };
                state.history_idx = Some(next);
                state.input = state.history[next].clone();
            }
        }
        (KeyCode::Down, _) => {
            match state.history_idx {
                Some(i) if i + 1 < state.history.len() => {
                    state.history_idx = Some(i + 1);
                    state.input = state.history[i + 1].clone();
                }
                _ => { state.history_idx = None; state.input.clear(); }
            }
        }
        (KeyCode::Esc, _) => { state.input.clear(); }
        _ => {}
    }
    Ok(())
}

pub async fn submit_prompt(state: &mut AppState, text: String) {
    state.run_started = Some(Instant::now());
    state.tokens = (0, 0, 0);
    state.tool_count = 0;
    state.lines.push(Line::Prompt { text: text.clone() });
    match state.client.run(text).await {
        Ok(resp) => {
            state.session_id = Some(resp.session_id);
            state.running = true;
            state.last_status = "thinking".into();
        }
        Err(e) => { state.lines.push(Line::Error { message: format!("/run: {e}") }); }
    }
}

async fn handle_slash(state: &mut AppState, cmd: &str) {
    let (head, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match head {
        "help" | "h" | "?" => {
            for l in HELP_TEXT.lines() {
                state.lines.push(Line::Info { text: l.to_string() });
            }
        }
        "clear" | "reset" => {
            state.lines.clear();
            state.tokens = (0, 0, 0);
            state.tool_count = 0;
            state.last_status = "idle".into();
            let _ = state.client.reset().await;
            state.lines.push(Line::Info { text: "memory cleared".into() });
        }
        "view" | "v" => {
            if let Some((path, _)) = state.latest_shot.clone() {
                open_in_preview(&path);
            } else {
                state.lines.push(Line::Info { text: "no screenshot yet".into() });
            }
        }
        "autoview" => {
            state.auto_view = !state.auto_view;
            state.lines.push(Line::Info {
                text: format!("auto-view {}", if state.auto_view { "ON — Preview opens on every screenshot" } else { "OFF" }),
            });
        }
        "diagnose" | "doctor" => {
            match state.client.diagnose().await {
                Ok(j) => {
                    state.lines.push(Line::Info { text: format!("diagnose: {j}") });
                }
                Err(e) => state.lines.push(Line::Error { message: format!("diagnose: {e}") }),
            }
        }
        "settings" | "config" | "prefs" => {
            state.settings_open = true;
            state.settings_cursor = 0;
        }
        "exit" | "quit" | "q" => { state.quit = true; }
        "" => {}
        _ => {
            // Unknown — pass through as a prompt? Safer: just say unknown.
            state.lines.push(Line::Info {
                text: format!("unknown command /{head} — try /help"),
            });
            let _ = rest;
        }
    }
}

const HELP_TEXT: &str = "  /help              show this help
  /settings          open the settings panel (↑↓ Enter to toggle, Esc to close)
  /clear             clear the screen and reset Lily's memory
  /view              open the latest screenshot in Preview
  /autoview          toggle: auto-open each new screenshot in Preview
  /diagnose          ask the daemon to test its own permissions
  /exit              quit
                     (keys) ⌃C cancel/quit · ⌃R clear · ↑↓ history · v view shot";

fn handle_lily_event(state: &mut AppState, ev: LilyEvent) {
    match ev {
        LilyEvent::Status { state: s } => { state.last_status = s; }
        LilyEvent::UserPrompt { .. } => { /* already echoed locally */ }
        LilyEvent::ToolCall { id, name, args } => {
            state.tool_count += 1;
            state.lines.push(Line::PendingTool {
                id,
                label: tool_label(&name, &args),
                frame: 0,
            });
        }
        LilyEvent::ToolResult { id, ok, summary, elapsed_ms } => {
            if let Some(pos) = state.lines.iter().rposition(|l| matches!(l, Line::PendingTool { id: i, .. } if i == &id)) {
                state.lines[pos] = Line::DoneTool { label: summary, elapsed_ms, ok };
            } else {
                state.lines.push(Line::DoneTool { label: summary, elapsed_ms, ok });
            }
        }
        LilyEvent::Screenshot { path, index } => {
            state.latest_shot = Some((path.clone(), index));
            state.lines.push(Line::Screenshot { path: path.clone(), index });
            if state.auto_view && state.last_auto_viewed != Some(index) {
                open_in_preview(&path);
                state.last_auto_viewed = Some(index);
            }
        }
        LilyEvent::Assistant { text } => {
            state.lines.push(Line::Assistant { text });
        }
        LilyEvent::Tokens { prompt, completion, total } => {
            state.tokens = (prompt, completion, total);
        }
        LilyEvent::Done { summary } => {
            state.lines.push(Line::DoneRun { summary });
            state.running = false;
            state.session_id = None;
            state.last_status = "idle".into();
        }
        LilyEvent::Error { message } => {
            state.lines.push(Line::Error { message });
        }
    }
}

fn tool_label(name: &str, args: &serde_json::Value) -> String {
    match name {
        "click" => {
            let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
            let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
            format!("click({x}, {y})")
        }
        "type_text" => {
            let s = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let preview: String = s.chars().take(48).collect();
            format!("type_text(\"{}{}\")", preview, if s.chars().count() > 48 { "…" } else { "" })
        }
        "key_press" => format!("key_press({})", args.get("combo").and_then(|v| v.as_str()).unwrap_or("")),
        "open_app" => format!("open_app(\"{}\")", args.get("name").and_then(|v| v.as_str()).unwrap_or("")),
        "applescript" => {
            let s = args.get("script").and_then(|v| v.as_str()).unwrap_or("");
            let one = s.split('\n').next().unwrap_or("");
            let preview: String = one.chars().take(56).collect();
            format!("applescript({preview}{}…)", if s.len() > 56 { "" } else { "" })
        }
        "shell" => {
            let s = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            let preview: String = s.chars().take(56).collect();
            format!("shell({preview}{}…)", if s.len() > 56 { "" } else { "" })
        }
        "screenshot" => "screenshot".into(),
        "scroll" => format!("scroll(dy={})", args.get("dy").and_then(|v| v.as_i64()).unwrap_or(0)),
        "wait" => format!("wait({:.1}s)", args.get("seconds").and_then(|v| v.as_f64()).unwrap_or(0.0)),
        "done" => "done".into(),
        other => other.to_string(),
    }
}
