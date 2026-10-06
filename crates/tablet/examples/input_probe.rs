//! cargo run -p photocraft-tablet --features input-probe --example input_probe
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let action = args.first().map(String::as_str).unwrap_or("");
    let initial = if ["--analyze", "--replay"].contains(&action) {
        let path = args.get(1).ok_or("Supply a capture JSON path")?;
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err("Capture exceeds 32 MiB".into());
        }
        Some(photocraft_tablet::input_probe::Capture::load(&std::fs::read(path).map_err(|e| e.to_string())?)?)
    } else {
        None
    };
    if action == "--analyze" {
        if let Some(capture) = initial {
            let gc = photocraft_tablet::input_probe::calibrate(&capture, "gc");
            let touch = photocraft_tablet::input_probe::calibrate(&capture, "touch");
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "streams": photocraft_tablet::input_probe::reports(&capture),
                    "gc": photocraft_tablet::input_probe::align_with_shift(&capture, "gc",gc.as_ref().map_or(0,|c|c.lag_ms)),
                    "gc_calibration": gc,
                    "touch_calibration": touch,
                    "touch": photocraft_tablet::input_probe::align_with_shift(&capture, "touch",touch.as_ref().map_or(0,|c|c.lag_ms))
                }))
                .map_err(|e| e.to_string())?
            );
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    photocraft_tablet::macos::run_input_probe(
        if action.starts_with("--") || action.is_empty() {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plan/evidence/raw-input")
        } else {
            action.into()
        },
        initial,
    )
    .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    return Err("The native capture window requires macOS; --analyze works on other platforms".into());
    #[cfg(target_os = "macos")]
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
