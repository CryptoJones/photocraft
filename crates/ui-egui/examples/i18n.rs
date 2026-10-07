//! Translator tooling (normally invoked through `cargo xtask i18n`).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result =
        photocraft_ui_egui::i18n::maintenance::run(&args, |path, text| photocraft_format::atomic_write(path, text.as_bytes()).map_err(|e| e.to_string()));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PhotoCraft translations: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
