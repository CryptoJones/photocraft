//! Coverage checks and scaffolding for translators; no desktop window is required.

use super::{
    catalog::{Catalog, parse_entries},
    config::{LanguageConfig, Manifest, PluralRule},
    native,
    runtime::LanguagePack,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Menu, widget, brush, blend-mode and generated preference source keys.
pub fn required_strings() -> Result<BTreeSet<String>, String> {
    let mut strings = BTreeSet::new();
    for &(path, label, _, _) in crate::menu_catalog::CATALOG {
        strings.extend(path.iter().map(|s| s.to_string()));
        strings.insert(label.to_string());
    }
    for &(_, label, path, _) in crate::menus::UI_COMMANDS {
        strings.extend(path.iter().map(|s| s.to_string()));
        strings.insert(label.to_string());
    }
    for command in photocraft_engine::command_specs().iter().filter(|c| !c.menu.is_empty()) {
        strings.extend(command.menu.iter().map(|s| s.to_string()));
        strings.insert(command.label.to_string());
    }
    strings.extend(crate::prefs_ui::required_translation_labels()?);
    strings.extend(crate::brush_panel::SECTIONS.iter().map(|(s, _)| s.to_string()));
    strings.extend(std::iter::once(photocraft_color::BlendMode::PassThrough).chain(photocraft_color::BlendMode::LAYER_MODES).map(|m| m.label().to_string()));
    scan_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut strings)?;
    strings.remove("---");
    Ok(strings)
}

fn scan_sources(dir: &Path, strings: &mut BTreeSet<String>) -> Result<(), String> {
    // The workspace source tree has a bounded number of directories. No recursive call stack.
    let mut pending = vec![dir.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = pending.pop() {
        visited = visited.saturating_add(1);
        if visited > 1024 {
            return Err("source tree exceeds 1024 directories".into());
        }
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && path.extension().is_some_and(|e| e == "rs")
                && !path.ends_with("maintenance.rs")
                && !path.ends_with("live_tests.rs")
                && !path.ends_with("pack_tests.rs")
            {
                let text = native::read_text(&path, 2 * 1024 * 1024)?.replace("\r\n", "\n");
                let code = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
                strings.extend(literals(code));
            }
        }
    }
    Ok(())
}

/// Collect literals inside tl!(...) expressions, including conditional English source keys.
fn literals(code: &str) -> Vec<String> {
    // Helpers returning owned translations use t directly rather than the borrowed tl macro.
    let code = code.replace("crate::i18n::t(", "tl!(");
    let mut found = Vec::new();
    let mut rest = code.as_str();
    while let Some(at) = rest.find("tl!(") {
        rest = rest.get(at.saturating_add(4)..).unwrap_or("");
        let mut depth = 1usize;
        let mut quote = None;
        let mut escaped = false;
        for (at, c) in rest.char_indices() {
            if let Some(start) = quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    if depth == 1
                        && let Some(literal) = rest.get(start..at.saturating_add(1))
                        && let Ok(value) = serde_json::from_str::<String>(literal)
                    {
                        found.push(value);
                    }
                    quote = None;
                }
            } else {
                match c {
                    '"' => quote = Some(at),
                    '(' => depth = depth.saturating_add(1),
                    ')' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            rest = rest.get(at.saturating_add(1)..).unwrap_or("");
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        if depth != 0 {
            break;
        }
    }
    found
}

pub fn report(dir: &Path) -> Result<serde_json::Value, String> {
    let inputs = native::read_inputs(dir)?.ok_or_else(|| format!("{}: manifest.json is missing", dir.display()))?;
    LanguagePack::build(inputs.manifest.clone(), &inputs.files)?;
    let required = required_strings()?;
    let mut languages = Vec::new();
    for language in &inputs.manifest.languages {
        let text = language.catalog.as_ref().and_then(|f| inputs.files.get(f)).map_or("", String::as_str);
        let catalog = Catalog::parse_checked(text, language.plural_rule)?;
        let missing: Vec<_> = required.iter().filter(|s| language.code != "en" && catalog.plain(s).is_none()).collect();
        languages.push(serde_json::json!({"code":language.code, "name":language.name, "entries":parse_entries(text).0.len(),
            "required":required.len(), "translated":required.len().saturating_sub(missing.len()),
            "completeMenus":language.complete_menus, "missing":missing}));
    }
    Ok(serde_json::json!({"schemaVersion":1,"languages":languages}))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

pub fn scaffold(dir: &Path, code: &str, name: &str, plural_rule: PluralRule, write_manifest: Writer) -> Result<(), String> {
    let (mut manifest, mut files) = match native::read_inputs(dir)? {
        Some(inputs) => {
            LanguagePack::build(inputs.manifest.clone(), &inputs.files)?;
            (inputs.manifest, inputs.files)
        }
        None => {
            (Manifest { schema: None, schema_version: 1, fallback_locale: "en".into(), reload_interval_ms: 1000, languages: Vec::new() }, Default::default())
        }
    };
    if manifest.languages.iter().any(|l| l.code == code) {
        return Err(format!("{code} is already registered"));
    }
    let file = format!("{code}.tsv");
    manifest.languages.push(LanguageConfig {
        code: code.into(),
        name: name.into(),
        catalog: Some(file.clone()),
        plural_rule,
        fallback: Some("en".into()),
        aliases: Vec::new(),
        complete_menus: false,
    });
    files.insert(file.clone(), String::new());
    LanguagePack::build(manifest.clone(), &files)?;
    // Scaffold TODO rows as comments; unfinished entries fall back rather than claiming coverage.
    let mut text = String::from(
        "# PhotoCraft translation catalog: context<TAB>English source<TAB>translation\n# Uncomment and translate TODO rows; keep placeholders and the trailing ellipsis.\n# Licensed under MIT OR Apache-2.0 (PhotoCraft contributors).\n",
    );
    for source in required_strings()? {
        text.push_str(&format!("# TODO\t{}\t\n", escape(&source)));
    }
    for (one, other) in [("{n} item", "{n} items"), ("{n} Layer", "{n} Layers"), ("{n} sample", "{n} samples")] {
        text.push_str(&format!("# @plural\t{one}|{other}\t{}\n", std::iter::repeat_n("{n} …", plural_rule.forms()).collect::<Vec<_>>().join("|")));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    use std::io::Write;
    let path = dir.join(&file);
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    output.write_all(text.as_bytes()).and_then(|()| output.sync_all()).map_err(|e| format!("{}: {e}", path.display()))?;
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    write_manifest(&dir.join("manifest.json"), &format!("{json}\n"))
}

/// Safe cross-platform atomic persistence provided by the tooling host.
pub type Writer = fn(&Path, &str) -> Result<(), String>;

pub fn run(args: &[String], write_manifest: Writer) -> Result<(), String> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut mode = "--report";
    let mut code = None;
    let mut name = None;
    let mut rule = PluralRule::OneOther;
    let mut json = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" | "--report" => mode = arg,
            "--json" => json = true,
            "--dir" => dir = PathBuf::from(args.next().ok_or("--dir requires a path")?),
            "--init" => {
                mode = "--init";
                code = Some(args.next().ok_or("--init requires a locale code")?);
            }
            "--name" => name = Some(args.next().ok_or("--name requires a native language name")?),
            "--plural-rule" => {
                rule =
                    serde_json::from_value(serde_json::Value::String(args.next().ok_or("--plural-rule requires a rule")?.clone())).map_err(|e| e.to_string())?
            }
            "--help" | "-h" => {
                println!(
                    "cargo xtask i18n [--check | --report] [--json] [--dir PATH]\ncargo xtask i18n --init CODE --name NAME [--plural-rule one_other|none|french|portuguese|russian|czech] [--dir PATH]"
                );
                return Ok(());
            }
            other => return Err(format!("unknown i18n argument {other:?}")),
        }
    }
    if mode == "--init" {
        return scaffold(&dir, code.ok_or("missing locale code")?, name.ok_or("--init requires --name")?, rule, write_manifest);
    }
    let report = report(&dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    } else {
        for language in report.get("languages").and_then(serde_json::Value::as_array).into_iter().flatten() {
            println!(
                "{}: {}/{} keys, {} entries",
                language.get("code").unwrap_or(&serde_json::Value::Null),
                language.get("translated").unwrap_or(&serde_json::Value::Null),
                language.get("required").unwrap_or(&serde_json::Value::Null),
                language.get("entries").unwrap_or(&serde_json::Value::Null)
            );
        }
    }
    if mode == "--check"
        && report.get("languages").and_then(serde_json::Value::as_array).is_some_and(|ls| {
            ls.iter().any(|l| {
                l.get("completeMenus").and_then(serde_json::Value::as_bool) == Some(true)
                    && l.get("missing").and_then(serde_json::Value::as_array).is_some_and(|m| !m.is_empty())
            })
        })
    {
        return Err("complete language catalogs have missing keys; run cargo xtask i18n --report --json for details".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_macro_sources_are_included() {
        assert_eq!(literals(r#"tl!(if yes { "Left" } else { "Right" }); tl!("Line\n{n}")"#), ["Left", "Right", "Line\n{n}"]);
        assert_eq!(literals(r#"tl!(if cfg!(target_os = "macos") { "Option" } else { "Alt" })"#), ["Option", "Alt"]);
        assert_eq!(literals(r#"crate::i18n::t("Owned helper label")"#), ["Owned helper label"]);
    }
}
