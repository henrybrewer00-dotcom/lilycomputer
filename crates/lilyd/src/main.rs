mod agent;
mod auth;
mod browser;
mod groq;
mod history;
mod routes;

use anyhow::Result;
use lily_core::{DAEMON_HOST, DAEMON_PORT};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, Mutex};
use tracing_subscriber::EnvFilter;

pub const MODEL: &str = "meta-llama/llama-4-scout-17b-16e-instruct";
pub const SERVICE_TIER: &str = "on_demand";

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    /// Optional — daemon now starts without it and accepts the key later via POST /set-key.
    pub groq_key: Arc<tokio::sync::RwLock<Option<String>>>,
    pub started: Instant,
    pub session: Arc<Mutex<Option<Session>>>,
    pub history: Arc<Mutex<history::History>>,
    pub events: broadcast::Sender<lily_core::protocol::Event>,
    pub http: reqwest::Client,
    pub browser: browser::BrowserBridge,
}

pub struct Session {
    pub id: String,
    pub cancel: tokio_util_cancel::Token,
}

pub mod tokio_util_cancel {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Default)]
    pub struct Token(Arc<AtomicBool>);
    impl Token {
        pub fn new() -> Self { Self(Arc::new(AtomicBool::new(false))) }
        pub fn cancel(&self) { self.0.store(true, Ordering::SeqCst); }
        pub fn is_cancelled(&self) -> bool { self.0.load(Ordering::SeqCst) }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Subcommands. Run BEFORE tracing init so warmup output is clean.
    if let Some(sub) = std::env::args().nth(1) {
        match sub.as_str() {
            "warmup" => return warmup().await,
            "diagnose" => return diagnose_cli().await,
            "--help" | "-h" | "help" => {
                println!("usage: lilyd [warmup|diagnose]\n");
                println!("  (no args) — run the HTTP daemon (this is what the LaunchAgent does)");
                println!("  warmup    — trigger the macOS Automation prompt for System Events");
                println!("              run this from a terminal in the assistant session, then click 'Allow'");
                println!("  diagnose  — test screencapture + Automation from a foreground context");
                return Ok(());
            }
            _ => {}
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_env("LILY_LOG").unwrap_or_else(|_| EnvFilter::new("info,axum=warn,tower=warn,hyper=warn,reqwest=warn")))
        .with_target(false)
        .init();

    let token = lily_core::config::load_or_create_token(true)?;
    // Don't bail when the key is missing — the daemon should still start so
    // the TUI can connect and prompt the user inline.
    let groq_key_opt = match lily_core::config::load_groq_key() {
        Ok(k) => Some(k),
        Err(e) => {
            tracing::warn!("starting without GROQ_API_KEY: {e}");
            None
        }
    };

    let (tx, _) = broadcast::channel(256);
    let history = history::History::load();
    tracing::info!("history: {} messages loaded", history.messages.len());
    let state = AppState {
        token,
        groq_key: Arc::new(tokio::sync::RwLock::new(groq_key_opt)),
        started: Instant::now(),
        session: Arc::new(Mutex::new(None)),
        history: Arc::new(Mutex::new(history)),
        events: tx,
        http: reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()?,
        browser: browser::BrowserBridge::new(),
    };

    let app = routes::router(state);
    let port: u16 = std::env::var("LILY_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(DAEMON_PORT);
    let addr: std::net::SocketAddr = format!("{DAEMON_HOST}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("lilyd listening on http://{addr}  model={MODEL}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Trigger TCC prompts for screencapture + System Events. Run this from a
/// terminal in the assistant's session: the daemon launched via LaunchAgent
/// usually can't show a TCC dialog because there's no foreground responsible
/// process, but `lilyd warmup` invoked from Terminal/iTerm CAN.
async fn warmup() -> Result<()> {
    println!("▸ Lily warmup — triggering macOS permission prompts");
    println!();

    // 1) screencapture (Screen Recording)
    println!("▸ testing screencapture (you may see a 'Screen Recording' prompt)");
    let tmp = std::env::temp_dir().join("lily-warmup.png");
    let out = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", tmp.to_str().unwrap()])
        .output()?;
    let sz = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    let _ = std::fs::remove_file(&tmp);
    if out.status.success() && sz > 1024 {
        println!("  ✓ screencapture ok ({} bytes)", sz);
    } else {
        println!("  ✗ screencapture failed/empty — grant Screen Recording in System Settings,");
        println!("    add ~/.local/bin/lilyd, then re-run `lilyd warmup`.");
    }

    // 2) System Events Apple Events (Automation → System Events)
    println!();
    println!("▸ testing AppleScript → System Events (may show an Automation prompt)");
    let r = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to get name of first process whose frontmost is true"#)
        .output()?;
    if r.status.success() {
        let name = String::from_utf8_lossy(&r.stdout).trim().to_string();
        println!("  ✓ Automation granted — frontmost process is '{}'", name);
    } else {
        let err = String::from_utf8_lossy(&r.stderr).trim().to_string();
        println!("  ✗ Automation denied: {}", err);
        println!("    Open System Settings → Privacy & Security → Automation,");
        println!("    expand the 'osascript' entry, toggle ON 'System Events'.");
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")
            .status();
    }

    // 3) Accessibility (keystroke/click) — attributed to osascript by macOS.
    println!();
    println!("▸ testing keystroke (Accessibility for osascript — may show a prompt)");
    let r = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to keystroke """#)
        .output()?;
    if r.status.success() {
        println!("  ✓ Accessibility granted — keystroke works");
    } else {
        let err = String::from_utf8_lossy(&r.stderr).trim().to_string();
        println!("  ✗ Accessibility denied: {}", err);
        println!();
        println!("  This is the perm that controls clicks/keystrokes (error -1002).");
        println!("  Open System Settings → Privacy & Security → Accessibility.");
        println!("  Click '+', press Cmd+Shift+G, paste:  /usr/bin/osascript");
        println!("  Press Enter, click Open, then toggle the new entry ON.");
        println!("  Re-run: ~/.local/bin/lilyd warmup");
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status();
    }

    println!();
    println!("▸ done. If all 3 checks passed, kickstart the LaunchAgent:");
    println!("    launchctl kickstart -k gui/$(id -u)/computer.lily.daemon");
    Ok(())
}

async fn diagnose_cli() -> Result<()> {
    // Same as warmup but doesn't try to be cute about it — just prints JSON.
    let report = run_diagnostic_checks().await;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub async fn run_diagnostic_checks() -> serde_json::Value {
    run_diagnostic_checks_with(None).await
}

pub async fn run_diagnostic_checks_with(bridge: Option<&crate::browser::BrowserBridge>) -> serde_json::Value {
    let mut report = serde_json::Map::new();
    if let Some(b) = bridge {
        report.insert("browser_extension".into(), serde_json::json!({
            "connected": b.is_connected().await,
        }));
    }

    // Probe multiple screencapture variants so we know whether ANY method works
    // for this user's session (e.g. a backgrounded user might still capture by
    // display ID even if the default fails).
    let variants: Vec<(&str, Vec<&str>)> = vec![
        ("default",  vec!["-x"]),
        ("display1", vec!["-x", "-D", "1"]),
        ("display2", vec!["-x", "-D", "2"]),
        ("main",     vec!["-x", "-m"]),
    ];
    let mut variant_results = serde_json::Map::new();
    let mut any_ok = false;
    let mut first_err = String::new();
    for (name, args) in &variants {
        let tmp = std::env::temp_dir().join(format!("lily-diag-{name}.png"));
        let mut full: Vec<&str> = args.clone();
        let path = tmp.to_string_lossy().into_owned();
        full.push(&path);
        let r = tokio::process::Command::new("/usr/sbin/screencapture")
            .args(&full)
            .output()
            .await;
        let size = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        let ok = r.as_ref().map(|o| o.status.success()).unwrap_or(false) && size > 1024;
        let err = match (&r, ok) {
            (Err(e), _) => format!("spawn: {e}"),
            (Ok(o), false) => {
                let s = String::from_utf8_lossy(&o.stderr).trim().to_string();
                if s.is_empty() { format!("0/small output ({size} bytes)") } else { s }
            }
            _ => String::new(),
        };
        let _ = std::fs::remove_file(&tmp);
        variant_results.insert(name.to_string(), serde_json::json!({
            "ok": ok, "size": size, "error": err.clone(),
        }));
        if ok { any_ok = true; }
        else if first_err.is_empty() { first_err = format!("{}: {}", name, err); }
    }
    report.insert("screen_recording".into(), serde_json::json!({
        "ok": any_ok,
        "error": if any_ok { String::new() } else { first_err },
        "variants": variant_results,
    }));

    // Automation (osascript → System Events Apple Events)
    let r = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to get name of first process whose frontmost is true"#)
        .output()
        .await;
    let (auto_ok, auto_err) = match r {
        Ok(o) if o.status.success() => (true, String::new()),
        Ok(o) => (false, String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, format!("spawn: {e}")),
    };
    report.insert("automation_system_events".into(), serde_json::json!({
        "ok": auto_ok, "error": auto_err,
    }));

    // Accessibility (keystroke — System Events can only synthesize input if
    // osascript has Accessibility). This is the perm responsible for -1002
    // errors.
    let r = tokio::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(r#"tell application "System Events" to keystroke """#)
        .output()
        .await;
    let (acc_ok, acc_err) = match r {
        Ok(o) if o.status.success() => (true, String::new()),
        Ok(o) => (false, String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, format!("spawn: {e}")),
    };
    report.insert("accessibility".into(), serde_json::json!({
        "ok": acc_ok, "error": acc_err,
    }));

    serde_json::Value::Object(report)
}
