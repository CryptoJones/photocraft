//! UI localisation. Stable ids resolve through embedded Fluent (`*.ftl`) catalogs. Legacy
//! English-string calls use generated `keys.tsv` to find those ids. Command ids, menu
//! paths used for logic, the control channel, the CLI and MCP never see translated text.
//!
//! # Adding a language
//! 1. Add `xx.ftl` translated from `en.ftl` and its stable ids.
//! 2. Add one row to [`LANGUAGES`] (code, native name, catalog).
//!
//! That is all: the Preferences dropdown, the system-locale match and the catalog tests (parse,
//! placeholders, plural forms) pick it up from the registry.
//!
//! # Looking strings up
//! - [`tr`]: a plain string. [`tr_ctx`]: when one English word needs different translations.
//! - [`tr_id`]: a command-id keyed string with the English label as fallback (menu items), so a
//!   translation survives rewording of the English text and can differ per command.
//! - [`trn`]: plural-aware (`{n}` is filled in). [`fmt`]: fill `{name}` placeholders after [`tr`];
//!   translators may reorder placeholders freely.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use unic_langid::{LanguageIdentifier, langid};

/// One supported UI language.
pub struct LangInfo {
    /// BCP 47 code, lowercase (`ja`, `zh-hans`, `pt-br`). Also the `interface.language` value.
    pub code: &'static str,
    /// The language's name in itself, shown in the Preferences dropdown.
    pub name: &'static str,
    /// Embedded Fluent catalog for stable message ids.
    pub fluent_source: &'static str,
    /// Must the catalog cover every menu string? (checked by the tests)
    pub complete_menus: bool,
    fluent: OnceLock<FluentBundle<FluentResource>>,
    simple: OnceLock<HashMap<String, String>>,
}

/// The registry. English first: it is the fallback and the source language.
pub static LANGUAGES: [LangInfo; 7] = [
    LangInfo { code: "en", name: "English", fluent_source: include_str!("locales/en/messages.ftl"), complete_menus: false, fluent: OnceLock::new(), simple: OnceLock::new() },
    LangInfo { code: "ja", name: "日本語", fluent_source: include_str!("locales/ja/messages.ftl"), complete_menus: true, fluent: OnceLock::new(), simple: OnceLock::new() },
    LangInfo {
        code: "zh-hans",
        name: "简体中文",
        fluent_source: include_str!("locales/zh-CN/messages.ftl"),
        complete_menus: true,
        fluent: OnceLock::new(),
        simple: OnceLock::new(),
    },
    // Traditional Chinese in the vocabulary used in Taiwan; `zh-TW`, `zh-HK`, `zh-MO` and `zh-Hant-*`
    // locales all resolve here (see `candidates`).
    LangInfo {
        code: "zh-hant",
        name: "繁體中文",
        fluent_source: include_str!("locales/zh-TW/messages.ftl"),
        complete_menus: true,
        fluent: OnceLock::new(),
        simple: OnceLock::new(),
    },
    LangInfo { code: "es", name: "Español", fluent_source: include_str!("locales/es/messages.ftl"), complete_menus: true, fluent: OnceLock::new(), simple: OnceLock::new() },
    LangInfo {
        code: "ru", name: "Русский", fluent_source: include_str!("locales/ru/messages.ftl"), complete_menus: true, fluent: OnceLock::new(), simple: OnceLock::new()
    },
    LangInfo { code: "cs", name: "Čeština", fluent_source: include_str!("locales/cs/messages.ftl"), complete_menus: true, fluent: OnceLock::new(), simple: OnceLock::new() },
];

impl LangInfo {
    fn fluent(&self) -> &FluentBundle<FluentResource> {
        self.fluent.get_or_init(|| {
            let locale: LanguageIdentifier = self.code.parse().unwrap_or_else(|_| langid!("en"));
            let mut bundle = FluentBundle::new_concurrent(vec![locale]);
            // Existing UI strings and tests expect plain text, without bidi isolate markers.
            bundle.set_use_isolating(false);
            let resource = match FluentResource::try_new(self.fluent_source.to_owned()) {
                Ok(resource) | Err((resource, _)) => resource,
            };
            let _ = bundle.add_resource(resource);
            bundle
        })
    }

    fn simple(&self) -> &HashMap<String, String> {
        self.simple.get_or_init(|| {
            let mut values = HashMap::new();
            for ((context, source), id) in keys() {
                if context == "@plural" || source.contains('{') {
                    continue;
                }
                if let Some(value) = render(self.fluent(), id, None) {
                    values.insert(id.clone(), value);
                }
            }
            values
        })
    }
}

type Key = (String, String);

fn keys() -> &'static HashMap<Key, String> {
    static KEYS: OnceLock<HashMap<Key, String>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys = HashMap::new();
        for line in include_str!("keys.tsv").lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let mut parts = line.split('\t');
            if let (Some(context), Some(source), Some(id), None) = (parts.next(), parts.next(), parts.next(), parts.next()) {
                keys.insert((unescape_key(context), unescape_key(source)), id.to_owned());
            }
        }
        keys
    })
}

fn unescape_key(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn key_id(context: &str, source: &str) -> Option<&'static str> {
    keys().get(&(context.to_owned(), source.to_owned())).map(String::as_str)
}

fn render(bundle: &FluentBundle<FluentResource>, id: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
    let pattern = bundle.get_message(id)?.value()?;
    let mut errors = Vec::new();
    let value = bundle.format_pattern(pattern, args, &mut errors);
    errors.is_empty().then(|| value.into_owned())
}

/// Format a stable Fluent message id with typed arguments, falling back to English.
pub fn msg(lang: Lang, id: &str, args: Option<&FluentArgs<'_>>) -> String {
    render(lang.0.fluent(), id, args).or_else(|| render(LANGUAGES[0].fluent(), id, args)).unwrap_or_else(|| id.to_owned())
}

/// Borrow a preformatted message by stable id. Use [`msg`] for messages with arguments.
pub fn id(lang: Lang, message_id: &str) -> &str {
    lang.0.simple().get(message_id).map(String::as_str).or_else(|| LANGUAGES[0].simple().get(message_id).map(String::as_str)).unwrap_or(message_id)
}

/// A language the UI can be shown in (a handle into [`LANGUAGES`]).
#[derive(Clone, Copy)]
pub struct Lang(&'static LangInfo);

impl std::fmt::Debug for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lang({})", self.0.code)
    }
}

impl PartialEq for Lang {
    fn eq(&self, other: &Self) -> bool {
        self.0.code == other.0.code
    }
}

impl Eq for Lang {}

impl Lang {
    pub const EN: Lang = Lang(&LANGUAGES[0]);

    pub fn code(self) -> &'static str {
        self.0.code
    }

    /// A language by its exact code.
    pub fn from_code(code: &str) -> Option<Lang> {
        LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code)).map(Lang)
    }

    /// Resolve the `interface.language` preference: a language code, or `auto` (and anything
    /// unknown, e.g. a code from a newer version) to follow the system locale.
    pub fn from_pref(pref: &str) -> Lang {
        Lang::from_code(pref).unwrap_or_else(system_lang)
    }

    /// Every registered language.
    pub fn all() -> impl Iterator<Item = Lang> {
        LANGUAGES.iter().map(Lang)
    }

    pub fn name(self) -> &'static str {
        self.0.name
    }

    /// Does this language's catalog claim to cover every menu string and `tl!` literal?
    pub fn complete_menus(self) -> bool {
        self.0.complete_menus
    }
}

/// Candidate language codes for a locale tag, most specific first: `zh_TW.UTF-8` →
/// `zh-tw`, `zh-hant`, `zh`.
fn candidates(tag: &str) -> Vec<String> {
    let base = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-").to_ascii_lowercase();
    let parts: Vec<&str> = base.split('-').filter(|p| !p.is_empty()).collect();
    let Some(&primary) = parts.first() else { return Vec::new() };
    let mut out = Vec::new();
    for n in (1..=parts.len()).rev() {
        out.push(parts[..n].join("-"));
    }
    if primary == "zh" && !parts.iter().any(|p| matches!(*p, "hans" | "hant")) {
        // Chinese by region when no script is given.
        let script = if parts.iter().any(|p| matches!(*p, "tw" | "hk" | "mo")) { "zh-hant" } else { "zh-hans" };
        out.insert(out.len() - 1, script.to_string());
    }
    out
}

/// The registered language for a locale tag such as `ja_JP.UTF-8`, `ja-JP`, `zh-TW`; `None` if
/// the language isn't supported. `C`/`POSIX` mean English.
pub fn lang_from_tag(tag: &str) -> Option<Lang> {
    let cands = candidates(tag);
    if matches!(cands.first().map(String::as_str), Some("c" | "posix")) {
        return Some(Lang::EN);
    }
    cands.iter().find_map(|c| Lang::from_code(c))
}

/// The system language (cached). English when it can't be determined.
pub fn system_lang() -> Lang {
    // Tests drive the UI by its English labels whatever the developer's locale is.
    if cfg!(test) {
        return Lang::EN;
    }
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(detect_system_lang)
}

#[cfg(not(target_arch = "wasm32"))]
fn detect_system_lang() -> Lang {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(l) = std::env::var(var).ok().filter(|v| !v.is_empty()).and_then(|v| lang_from_tag(&v)) {
            return l;
        }
    }
    // Apps started from the Finder don't inherit LANG: use the macOS preferred-languages list.
    // The absolute path keeps a `defaults` earlier on PATH from running; any failure means English.
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("/usr/bin/defaults").args(["read", "-g", "AppleLanguages"]).output()
        && out.status.success()
        && let Some(l) = first_supported(&String::from_utf8_lossy(&out.stdout))
    {
        return l;
    }
    // Windows sets no LANG: fall back to the OS locale the text engine already reads for its CJK
    // font order (`HKCU\Control Panel\International` › `LocaleName`, e.g. `zh-TW`; on macOS the
    // preferences plist). `PHOTOCRAFT_LOCALE` overrides it there too.
    if let Some(l) = photocraft_text::cjk::ui_locale().and_then(lang_from_tag) {
        return l;
    }
    Lang::EN
}

/// The first supported language in a `defaults read` list like `(\n    "ja-JP",\n    "en-US"\n)`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn first_supported(list: &str) -> Option<Lang> {
    list.split(['(', ')', ',', '"', '\n']).map(str::trim).filter(|s| !s.is_empty()).find_map(lang_from_tag)
}

#[cfg(target_arch = "wasm32")]
fn detect_system_lang() -> Lang {
    Lang::EN
}

thread_local! {
    /// Index into [`LANGUAGES`] for the UI being drawn on this thread.
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

/// Set the UI language for drawing (the shell calls this once per frame from the preference), so
/// widgets can translate without every call site carrying a language around.
pub fn set_current(lang: Lang) {
    let i = LANGUAGES.iter().position(|l| l.code == lang.code()).unwrap_or(0);
    CURRENT.with(|current| current.set(i));
}

/// The language the UI is drawn in.
pub fn current() -> Lang {
    CURRENT.with(|current| Lang(LANGUAGES.get(current.get()).unwrap_or(&LANGUAGES[0])))
}

/// Temporarily draw in another language, restoring the previous one even on unwinding.
/// Preferences previews use this without changing the saved setting or other dialogs.
pub fn with_language<R>(lang: Lang, draw: impl FnOnce() -> R) -> R {
    let _restore = language_scope(lang);
    draw()
}

/// Keep the language active until this guard is dropped on the drawing thread.
#[must_use]
pub fn language_scope(lang: Lang) -> impl Drop {
    struct Restore {
        previous: Lang,
        _thread: std::marker::PhantomData<std::rc::Rc<()>>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            set_current(self.previous);
        }
    }
    let restore = Restore { previous: current(), _thread: std::marker::PhantomData };
    set_current(lang);
    restore
}

/// Apply a committed language to this context. Font caches are rebuilt only on a language
/// change, so Han glyphs follow the selected script even when the previous font covered them.
pub fn sync_context(ctx: &egui::Context, language: &str) {
    let lang = Lang::from_pref(language);
    set_current(lang);
    let id = egui::Id::new("photocraft-ui-language");
    let changed = ctx.data(|data| data.get_temp::<Lang>(id) != Some(lang));
    if changed {
        ctx.data_mut(|data| data.insert_temp(id, lang));
        crate::theme::install_fonts(ctx);
        ctx.request_repaint();
    }
}

/// Does `lang` have a catalog entry for this plain string? (English never does: it is the source.)
pub fn has(lang: Lang, s: &str) -> bool {
    key_id("", s).is_some_and(|id| lang.0.fluent().get_message(id).is_some())
}

/// Translate an English UI string into the current language ([`tr`] with [`current`]).
pub fn t(s: &str) -> &str {
    tr(current(), s)
}

/// Translate an English UI string; unknown strings come back unchanged.
pub fn tr(lang: Lang, s: &str) -> &str {
    if s.contains('{') {
        return s;
    }
    if let Some(id) = key_id("", s)
        && let Some(value) = lang.0.simple().get(id)
    {
        return value;
    }
    s
}

/// Like [`tr`], for an English string that needs a disambiguating `context`.
pub fn tr_ctx<'a>(lang: Lang, context: &str, s: &'a str) -> &'a str {
    if s.contains('{') {
        return s;
    }
    if let Some(id) = key_id(context, s)
        && let Some(value) = lang.0.simple().get(id)
    {
        return value;
    }
    tr(lang, s)
}

/// A string keyed by its command id, falling back to the translation of the English `label`.
pub fn tr_id<'a>(lang: Lang, id: &str, label: &'a str) -> &'a str {
    if lang != Lang::EN
        && !label.contains('{')
        && let Some(message_id) = key_id("@id", id)
        && let Some(value) = lang.0.simple().get(message_id)
    {
        return value;
    }
    tr(lang, label)
}

/// Fill `{name}` placeholders. Unknown placeholders are left as written.
pub fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    let lang = current();
    if let Some(id) = key_id("", template) {
        let mut fluent_args = FluentArgs::new();
        for (name, value) in args {
            fluent_args.set(*name, *value);
        }
        if let Some(value) = render(lang.0.fluent(), id, Some(&fluent_args)).or_else(|| render(LANGUAGES[0].fluent(), id, Some(&fluent_args))) {
            return value;
        }
    }
    let mut out = template.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// A plural-aware message: `one`/`other` are the English forms (with `{n}` where the count goes).
pub fn trn(lang: Lang, n: u64, one: &str, other: &str) -> String {
    if let (Some(id), Ok(count)) = (key_id("@plural", &format!("{one}|{other}")), i64::try_from(n)) {
        let mut args = FluentArgs::new();
        args.set("n", count);
        if let Some(value) = render(lang.0.fluent(), id, Some(&args)).or_else(|| render(LANGUAGES[0].fluent(), id, Some(&args))) {
            return value;
        }
    }
    let text = if n == 1 { one } else { other };
    text.replace("{n}", &n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language(code: &str) -> Lang {
        Lang::from_code(code).expect("registered language")
    }

    #[test]
    fn locale_resolution_preserves_chinese_script() {
        assert_eq!(lang_from_tag("ja_JP.UTF-8"), Some(language("ja")));
        assert_eq!(lang_from_tag("cs-CZ"), Some(language("cs")));
        assert_eq!(lang_from_tag("en-US"), Some(Lang::EN));
        assert_eq!(lang_from_tag("C"), Some(Lang::EN));
        assert_eq!(lang_from_tag("fr-FR"), None);
        for tag in ["zh-TW", "zh-HK", "zh-MO", "zh-Hant-TW"] {
            assert_eq!(lang_from_tag(tag), Some(language("zh-hant")));
        }
        for tag in ["zh-CN", "zh-SG", "zh-Hans", "zh"] {
            assert_eq!(lang_from_tag(tag), Some(language("zh-hans")));
        }
        assert_eq!(first_supported("(\n    \"ja-JP\",\n    \"en-US\"\n)"), Some(language("ja")));
        assert_eq!(Lang::from_pref("unknown"), Lang::EN);
    }

    #[test]
    fn fluent_static_lookup_and_english_fallback() {
        let layer = key_id("", "Layer").expect("Layer id");
        assert_eq!(id(language("ja"), layer), "レイヤー");
        assert_eq!(msg(language("ja"), layer, None), "レイヤー");
        assert_eq!(id(Lang::EN, layer), "Layer");
        assert_eq!(tr(language("zh-hans"), "Layer"), "图层");
        assert_eq!(tr(language("ja"), "absent source"), "absent source");
        assert_eq!(msg(language("ja"), "absent-id", None), "absent-id");
        assert_eq!(tr_id(Lang::EN, "edit.purge.undo", "Undo"), "Undo");
        assert_eq!(tr_id(language("cs"), "select.all", "All"), "Vybrat vše");
        assert_eq!(tr_ctx(language("ja"), "unknown", "Layer"), "レイヤー");
    }

    #[test]
    fn fluent_formats_variables_and_plural_categories() {
        set_current(language("ja"));
        assert_eq!(fmt(tr(current(), "Opening {name}…"), &[("name", "image.psd")]), "image.psd を開いています…");
        set_current(Lang::EN);
        assert_eq!(fmt("{b} before {a}", &[("a", "x"), ("b", "y")]), "y before x");
        assert_eq!(trn(language("ja"), 7, "{n} item", "{n} items"), "7 件");
        assert_eq!(trn(language("zh-hant"), 7, "{n} item", "{n} items"), "7 個項目");
        for (n, expected) in [(1, "1 элемент"), (2, "2 элемента"), (5, "5 элементов"), (11, "11 элементов"), (21, "21 элемент")]
        {
            assert_eq!(trn(language("ru"), n, "{n} item", "{n} items"), expected);
        }
        for (n, expected) in [(1, "1 položka"), (3, "3 položky"), (5, "5 položek")] {
            assert_eq!(trn(language("cs"), n, "{n} item", "{n} items"), expected);
        }
    }

    #[test]
    fn current_language_is_thread_local() {
        set_current(language("ja"));
        assert_eq!(current(), language("ja"));
        assert_eq!(std::thread::spawn(current).join().expect("thread joined"), Lang::EN);
        set_current(Lang::EN);
    }

    #[test]
    fn complete_catalogs_cover_menu_sources() {
        let mut sources = std::collections::BTreeSet::new();
        for &(path, label, _, _) in crate::menu_catalog::CATALOG {
            sources.extend(path.iter().copied());
            sources.insert(label);
        }
        for &(_, label, path, _) in crate::menus::UI_COMMANDS {
            sources.extend(path.iter().copied());
            sources.insert(label);
        }
        for command in photocraft_engine::command_specs().iter().filter(|command| !command.menu.is_empty()) {
            sources.extend(command.menu.iter().copied());
            sources.insert(command.label);
        }
        sources.remove("---");
        for lang in Lang::all().filter(|lang| lang.complete_menus()) {
            let missing: Vec<_> = sources.iter().filter(|source| !has(lang, source)).collect();
            assert!(missing.is_empty(), "{}: missing menu messages: {missing:#?}", lang.code());
        }
    }

    #[test]
    fn stable_ids_resolve_in_all_complete_catalogs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut ids = std::collections::BTreeSet::new();
        let mut dirs = vec![root];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).expect("source directory").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let code = std::fs::read_to_string(path).expect("Rust source");
                    let code = code.split("#[cfg(test)]\nmod ").next().unwrap_or("");
                    let mut rest = code;
                    while let Some(at) = rest.find("tl_id!(\"") {
                        rest = &rest[at + 8..];
                        if let Some(end) = rest.find('"')
                            && rest.get(end + 1..end + 2) == Some(")")
                        {
                            ids.insert(rest[..end].to_owned());
                        }
                    }
                }
            }
        }
        assert!(ids.len() > 300, "found only {} stable ids", ids.len());
        for lang in Lang::all().filter(|lang| lang.complete_menus()) {
            let missing: Vec<_> = ids.iter().filter(|id| lang.0.fluent().get_message(id).is_none()).collect();
            assert!(missing.is_empty(), "{}: missing Fluent ids: {missing:#?}", lang.code());
        }
    }
}
