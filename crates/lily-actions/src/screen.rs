use crate::ToolOutcome;
use anyhow::{Context, Result};
use fast_image_resize as fir;
use image::ImageEncoder;
use std::io::Cursor;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;

const TARGET_WIDTH: u32 = 1280;
static SHOT_COUNTER: AtomicU32 = AtomicU32::new(0);

pub async fn take() -> Result<ToolOutcome> {
    let raw = capture_png().await?;
    let resized = resize_to_width(&raw, TARGET_WIDTH).unwrap_or(raw);
    let (w, h) = png_dims(&resized).unwrap_or((0, 0));

    // Save a copy to /Users/Shared/lily/shots/ so the client can display it.
    let idx = SHOT_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    let path = save_for_client(&resized, idx);

    let summary = format!("screenshot #{idx} ({w}x{h}, {} KB)", resized.len() / 1024);
    let content = format!(
        "Screen captured at {w}x{h} (saved to {}). The image is attached as the next user message. \
         Coordinates in this screenshot map 1:1 to click() coordinates.",
        path.as_deref().unwrap_or("(unsaved)")
    );

    let mut out = ToolOutcome::ok(summary, content).with_image(resized);
    if let Some(p) = path {
        out = out.with_screenshot_path(p);
    }
    Ok(out)
}

fn save_for_client(png: &[u8], idx: u32) -> Option<String> {
    let dir = std::path::PathBuf::from("/Users/Shared/lily/shots");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("save_for_client mkdir: {e}");
        return None;
    }
    let numbered = dir.join(format!("screen-{idx:04}.png"));
    let latest = dir.join("latest.png");
    let tmp = dir.join(".latest.tmp.png");
    if let Err(e) = std::fs::write(&numbered, png) {
        tracing::warn!("save_for_client write numbered: {e}");
        return None;
    }
    if let Err(e) = std::fs::write(&tmp, png) {
        tracing::warn!("save_for_client write tmp: {e}");
        return None;
    }
    if let Err(e) = std::fs::rename(&tmp, &latest) {
        tracing::warn!("save_for_client rename latest: {e}");
    }
    Some(numbered.to_string_lossy().into_owned())
}

async fn capture_png() -> Result<Vec<u8>> {
    // Try several screencapture variants. Default (CGDisplayCreateImage) may
    // fail on a backgrounded Fast-User-Switched session, but specific display
    // IDs or window-list capture sometimes still work depending on macOS
    // version and whether the user's WindowServer was ever foregrounded.
    let methods: Vec<(&str, Vec<String>)> = vec![
        ("default",  ["-x", "-C", "-t", "png"].iter().map(|s| s.to_string()).collect()),
        ("display1", ["-x", "-C", "-D", "1", "-t", "png"].iter().map(|s| s.to_string()).collect()),
        ("display2", ["-x", "-C", "-D", "2", "-t", "png"].iter().map(|s| s.to_string()).collect()),
        ("main",     ["-x", "-C", "-m", "-t", "png"].iter().map(|s| s.to_string()).collect()),
        // -o = no shadow (irrelevant), -r = no shadow on window capture (irrelevant)
        // -k captures the cursor's "key" press state — skip.
        // -B captures from a specific bundle ID. Not useful generically.
    ];

    let pid = std::process::id();
    let mut errors: Vec<String> = Vec::new();

    for (name, args) in &methods {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = std::env::temp_dir().join(format!("lily-shot-{name}-{pid}-{nonce}.png"));
        let path_str = tmp.to_string_lossy().into_owned();

        let mut full_args: Vec<String> = args.clone();
        full_args.push(path_str);

        let out = Command::new("/usr/sbin/screencapture")
            .args(&full_args)
            .output()
            .await
            .context("spawn screencapture")?;

        if out.status.success() {
            if let Ok(bytes) = std::fs::read(&tmp) {
                let _ = std::fs::remove_file(&tmp);
                if bytes.len() > 1024 {
                    tracing::info!("screencapture variant '{}' succeeded ({} bytes)", name, bytes.len());
                    return Ok(bytes);
                }
            }
        }
        let _ = std::fs::remove_file(&tmp);

        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        errors.push(format!("{}: {}", name, if err.is_empty() { "0-byte/missing output".into() } else { err }));
    }

    let combined = errors.join("\n  ");
    let backgrounded_signal = combined.contains("could not create image from display")
        || combined.contains("could not record image");
    if backgrounded_signal {
        anyhow::bail!(
            "VISION_UNAVAILABLE: tried 4 screencapture variants from the daemon, all failed. This is macOS refusing display capture for a backgrounded Fast-User-Switched session.\n\
             Use describe_screen, read_ui, get_text, click_element instead — they read the accessibility tree and work without a compositor. Do not call screenshot() again this turn.\n\
             Variants tried:\n  {}",
            combined
        );
    }
    anyhow::bail!(
        "screencapture failed (all 4 variants). Could be Screen Recording perm missing, or display unavailable.\n\
         Variants tried:\n  {}\n\
         To check perms: in rhettbrewer's terminal run `~/.local/bin/lilyd warmup`.",
        combined
    );
}

fn png_dims(png: &[u8]) -> Result<(u32, u32)> {
    let img = image::load_from_memory(png)?;
    Ok((img.width(), img.height()))
}

fn resize_to_width(png: &[u8], target_w: u32) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png)?;
    let (w, h) = (img.width(), img.height());
    if w <= target_w {
        return Ok(png.to_vec());
    }
    let target_h = ((h as f32) * (target_w as f32) / (w as f32)).round() as u32;

    let rgba = img.to_rgba8();
    let src = fir::images::Image::from_vec_u8(
        w,
        h,
        rgba.into_raw(),
        fir::PixelType::U8x4,
    )?;
    let mut dst = fir::images::Image::new(target_w, target_h, fir::PixelType::U8x4);
    let mut resizer = fir::Resizer::new();
    resizer.resize(&src, &mut dst, None)?;

    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        Cursor::new(&mut out),
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    encoder.write_image(
        dst.buffer(),
        target_w,
        target_h,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}
