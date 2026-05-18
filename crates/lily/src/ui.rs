use crate::app::{AppState, Line};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line as TLine, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

const ACCENT: Color = Color::Rgb(236, 72, 153); // hot pink — Lily

pub fn draw(f: &mut Frame, state: &AppState) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),         // top status / header band
            Constraint::Min(3),            // event log
            Constraint::Length(3),         // input
            Constraint::Length(1),         // footer
        ])
        .split(size);

    draw_header(f, chunks[0], state);
    draw_log(f, chunks[1], state);
    draw_input(f, chunks[2], state);
    draw_footer(f, chunks[3], state);
}

fn draw_header(f: &mut Frame, area: Rect, state: &AppState) {
    let dot = match state.last_status.as_str() {
        "thinking" => Span::styled("●", Style::default().fg(Color::Yellow)),
        "acting"   => Span::styled("●", Style::default().fg(Color::Green)),
        _          => Span::styled("●", Style::default().fg(Color::DarkGray)),
    };
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let title = vec![
        Span::styled(" lily computer ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(format!("· {user} "), Style::default().fg(Color::DarkGray)),
        dot,
        Span::raw(" "),
        Span::styled(state.last_status.clone(), Style::default().fg(Color::Gray)),
    ];
    let p = Paragraph::new(TLine::from(title));
    f.render_widget(p, area);
}

fn draw_log(f: &mut Frame, area: Rect, state: &AppState) {
    let spinners = ["◐", "◓", "◑", "◒"];
    let mut tlines: Vec<TLine> = Vec::new();

    for line in &state.lines {
        match line {
            Line::Banner { text } => {
                tlines.push(TLine::from(Span::styled(text.clone(), Style::default().fg(ACCENT))));
            }
            Line::Prompt { text } => {
                tlines.push(TLine::from(""));
                tlines.push(TLine::from(vec![
                    Span::styled("▸ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                    Span::styled(text.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]));
            }
            Line::PendingTool { label, frame, .. } => {
                let glyph = spinners[*frame % spinners.len()];
                tlines.push(TLine::from(vec![
                    Span::raw("  "),
                    Span::styled(glyph.to_string(), Style::default().fg(Color::Yellow)),
                    Span::raw(" "),
                    Span::styled(label.clone(), Style::default().fg(Color::Gray)),
                ]));
            }
            Line::DoneTool { label, elapsed_ms, ok } => {
                let glyph = if *ok { "●" } else { "✗" };
                let color = if *ok { Color::Green } else { Color::Red };
                tlines.push(TLine::from(vec![
                    Span::raw("  "),
                    Span::styled(glyph.to_string(), Style::default().fg(color)),
                    Span::raw(" "),
                    Span::styled(label.clone(), Style::default().fg(Color::Gray)),
                    Span::raw("  "),
                    Span::styled(format_ms(*elapsed_ms), Style::default().fg(Color::DarkGray)),
                ]));
            }
            Line::Assistant { text } => {
                tlines.push(TLine::from(vec![
                    Span::raw("  "),
                    Span::styled("◆ ", Style::default().fg(ACCENT)),
                    Span::styled(text.clone(), Style::default().fg(Color::White)),
                ]));
            }
            Line::DoneRun { summary } => {
                tlines.push(TLine::from(""));
                tlines.push(TLine::from(vec![
                    Span::styled("✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(summary.clone(), Style::default().fg(Color::White)),
                ]));
                tlines.push(TLine::from(""));
            }
            Line::Screenshot { path, index } => {
                tlines.push(TLine::from(vec![
                    Span::raw("  "),
                    Span::styled("📷 ", Style::default().fg(ACCENT)),
                    Span::styled(format!("screenshot #{index}"), Style::default().fg(Color::White)),
                    Span::styled("  ·  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(path.clone(), Style::default().fg(Color::DarkGray)),
                    Span::styled("  ·  press v to view", Style::default().fg(Color::DarkGray)),
                ]));
            }
            Line::Info { text } => {
                tlines.push(TLine::from(vec![
                    Span::raw("  "),
                    Span::styled(text.clone(), Style::default().fg(Color::Cyan)),
                ]));
            }
            Line::Error { message } => {
                tlines.push(TLine::from(vec![
                    Span::raw("  "),
                    Span::styled("✗ ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                    Span::styled(message.clone(), Style::default().fg(Color::Red)),
                ]));
            }
        }
    }

    // Auto-scroll: pick the offset so the bottom of `tlines` is visible.
    let visible_h = area.height.saturating_sub(2) as usize; // borders
    let total = tlines.len();
    let scroll: u16 = if total > visible_h { (total - visible_h) as u16 } else { 0 };

    let block = Block::default().borders(Borders::NONE);
    let p = Paragraph::new(tlines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(p, area);
}

fn draw_input(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray));

    let cursor = if (std::time::Instant::now().elapsed().as_millis() / 500) % 2 == 0 { "▏" } else { " " };
    let prompt_color = if state.running { Color::DarkGray } else { ACCENT };

    let p = Paragraph::new(TLine::from(vec![
        Span::styled(" > ", Style::default().fg(prompt_color).add_modifier(Modifier::BOLD)),
        Span::styled(state.input.clone(), Style::default().fg(Color::White)),
        Span::styled(cursor.to_string(), Style::default().fg(Color::Gray)),
    ]))
    .block(block);
    f.render_widget(p, area);
}

fn draw_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let elapsed = state
        .run_started
        .map(|t| t.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    let mut parts: Vec<Span> = vec![
        Span::styled(" ⏱ ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.1}s", elapsed), Style::default().fg(Color::Gray)),
        Span::styled("  · ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{} tools", state.tool_count), Style::default().fg(Color::Gray)),
        Span::styled("  · ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{} tok", state.tokens.2), Style::default().fg(Color::Gray)),
    ];
    if let Some((_, idx)) = &state.latest_shot {
        parts.push(Span::styled("  · ", Style::default().fg(Color::DarkGray)));
        parts.push(Span::styled(format!("📷 #{idx}"), Style::default().fg(ACCENT)));
        parts.push(Span::styled(" v", Style::default().fg(Color::Gray)));
        if state.auto_view {
            parts.push(Span::styled(" (auto)", Style::default().fg(Color::DarkGray)));
        }
    }
    parts.push(Span::styled("   ⌃C ", Style::default().fg(Color::DarkGray)));
    parts.push(Span::styled(if state.running { "cancel" } else { "quit" }, Style::default().fg(Color::Gray)));
    parts.push(Span::styled("  ⌃R ", Style::default().fg(Color::DarkGray)));
    parts.push(Span::styled("clear", Style::default().fg(Color::Gray)));
    parts.push(Span::styled("  /help", Style::default().fg(Color::DarkGray)));
    f.render_widget(Paragraph::new(TLine::from(parts)), area);
}

fn format_ms(ms: u64) -> String {
    if ms >= 1000 { format!("{:.2}s", ms as f32 / 1000.0) } else { format!("{}ms", ms) }
}
