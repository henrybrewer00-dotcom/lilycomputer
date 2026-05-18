mod app;
mod client;
mod settings;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;

pub const BANNER: &str = r#"   __    _ __         ______                            __
  / /   (_) /_  __   / ____/___  ____ ___  ____  __  __/ /____  _____
 / /   / / / / / /  / /   / __ \/ __ `__ \/ __ \/ / / / __/ _ \/ ___/
/ /___/ / / /_/ /  / /___/ /_/ / / / / / / /_/ / /_/ / /_/  __/ /
\____/_/_/\__, /   \____/\____/_/ /_/ /_/ .___/\__,_/\__/\___/_/
         /____/                        /_/"#;

struct Args {
    once: bool,
    prompt: Option<String>,
    show_help: bool,
}

fn parse_args() -> Args {
    let mut a = Args { once: false, prompt: None, show_help: false };
    let mut rest: Vec<String> = Vec::new();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--once" | "-1" => a.once = true,
            "--help" | "-h" => a.show_help = true,
            _ => rest.push(arg),
        }
    }
    if !rest.is_empty() {
        a.prompt = Some(rest.join(" "));
    }
    a
}

fn print_cli_help() {
    eprintln!();
    eprintln!("{BANNER}");
    eprintln!();
    eprintln!("usage: lily [--once] [\"prompt\"]");
    eprintln!();
    eprintln!("  lily                    launch the TUI and stay open");
    eprintln!("  lily \"do something\"     run the prompt at startup, then stay open");
    eprintln!("  lily --once \"do X\"      run the prompt, then exit");
    eprintln!("  lily --help             show this message");
    eprintln!();
    eprintln!("inside the TUI, /help lists slash commands.");
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = parse_args();
    if args.show_help {
        print_cli_help();
        return Ok(());
    }

    let token = lily_core::config::load_or_create_token(false)
        .map_err(|e| anyhow::anyhow!("could not load lily token (is lilyd running?): {e}"))?;

    let client = client::DaemonClient::new(lily_core::daemon_base_url(), token);
    // Quick health probe up-front for a clearer error.
    let health = match client.health().await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("\n  lily: cannot reach lilyd at {} — {e}\n", lily_core::daemon_base_url());
            eprintln!("  Is lilyd running on this machine?");
            eprintln!("  Try:  launchctl print gui/$(id -u)/computer.lily.daemon");
            eprintln!("  Or run `~/.local/bin/lilyd` directly to debug.");
            std::process::exit(2);
        }
    };

    // If the daemon hasn't been given a Groq key yet, prompt inline before
    // entering the TUI. The installer can skip this step intentionally; this
    // is where we pick it up.
    if !health.groq_configured {
        eprintln!();
        eprintln!("  Lily needs your Groq API key to think. Free key at:");
        eprintln!("  https://console.groq.com/keys");
        eprintln!();
        eprint!("  paste key (hidden), or Enter to skip: ");
        use std::io::Write;
        std::io::stderr().flush().ok();
        let key = read_password_line().unwrap_or_default();
        let key = key.trim().to_string();
        if key.starts_with("gsk_") && key.len() >= 20 {
            match client.set_key(key).await {
                Ok(_) => eprintln!("  ✓ key saved.\n"),
                Err(e) => eprintln!("  ✗ couldn't save key: {e}\n"),
            }
        } else if !key.is_empty() {
            eprintln!("  that doesn't look like a Groq key — continuing without it.\n");
        } else {
            eprintln!("  (skipped — Lily can't think until you set GROQ_API_KEY)\n");
        }
    }

    // --once: run prompt, stream until Done, exit.
    if args.once {
        let Some(prompt) = args.prompt.clone() else {
            eprintln!("lily --once requires a prompt");
            std::process::exit(2);
        };
        return run_once(&client, prompt).await;
    }

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut events = EventStream::new();
    let mut state = app::AppState::new(client);
    if let Some(p) = args.prompt {
        app::submit_prompt(&mut state, p).await;
    }
    let res = app::run(&mut terminal, &mut state, &mut events).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("lily exited with error: {e}");
        std::process::exit(1);
    }
    Ok(())
}

/// Read a line from stdin with terminal echo disabled (best-effort via stty).
fn read_password_line() -> std::io::Result<String> {
    use std::io::BufRead;
    let _ = std::process::Command::new("stty").arg("-echo").status();
    let mut s = String::new();
    let res = std::io::stdin().lock().read_line(&mut s);
    let _ = std::process::Command::new("stty").arg("echo").status();
    eprintln!();
    res?;
    Ok(s.trim_end_matches('\n').to_string())
}

async fn run_once(client: &client::DaemonClient, prompt: String) -> Result<()> {
    use futures_util::StreamExt;
    use lily_core::protocol::Event;

    let mut stream = client.stream().await?;
    let _ = client.run(prompt).await?;
    while let Some(evt) = stream.next().await {
        match evt? {
            Event::ToolCall { name, args, .. } => {
                let preview = args.to_string();
                let preview: String = preview.chars().take(140).collect();
                println!("  ▶ {name}({preview})");
            }
            Event::ToolResult { ok, summary, elapsed_ms, .. } => {
                let mark = if ok { "✓" } else { "✗" };
                println!("    {mark} {summary}  ({elapsed_ms}ms)");
            }
            Event::Screenshot { path, index } => {
                println!("    📷 #{index} → {path}");
            }
            Event::Assistant { text } => println!("  ◆ {text}"),
            Event::Done { summary } => { println!("\n✓ {summary}"); break; }
            Event::Error { message } => { eprintln!("\n✗ {message}"); std::process::exit(1); }
            _ => {}
        }
    }
    Ok(())
}
