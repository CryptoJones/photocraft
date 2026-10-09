//! UI localisation. Stable ids resolve through embedded Fluent (`*.ftl`) catalogs. Legacy
//! English-string calls use generated `keys.tsv` to find those ids. Command ids, menu
//! paths used for logic, the control channel, the CLI and MCP never see translated text.
//!
//! # Adding a language
//! 1. Add `locales/xx/messages.ftl` translated from `locales/en/messages.ftl` and its stable ids.
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

mod catalog;
mod system;
pub use system::system_lang;
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod maintenance;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod runtime;

use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use unic_langid::{LanguageIdentifier, langid};

/// An embedded language. Runtime metadata is described by [`config::LanguageConfig`].
pub struct LangInfo {
    pub code: &'static str,
    pub name: &'static str,
    pub source: &'static str,
    pub plural: PluralRule,
    pub complete_menus: bool,
    fluent: OnceLock<FluentBundle<FluentResource>>,
    simple: OnceLock<HashMap<String, String>>,
}

/// French: 0 and 1 take the singular, everything else the plural.
fn plural_fr(n: u64) -> usize {
    usize::from(n > 1)
}

/// Portuguese: 0 and 1 take the singular, everything else the plural.
fn plural_pt(n: u64) -> usize {
    usize::from(n > 1)
}

/// Polish: 1 → one; 2–4, except 12–14 → few; everything else → many.
fn plural_polish(n: u64) -> usize {
    let last = n % 10;
    let last_two = n % 100;
    if n == 1 {
        0
    } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
        1
    } else {
        2
    }
}

/// The registry. English first: it is the fallback and the source language.
pub static LANGUAGES: [LangInfo; 15] = [
    LangInfo { code: "en", name: "English", source: "", plural: plural_one_other, complete_menus: false, catalog: OnceLock::new() },
    LangInfo { code: "ja", name: "日本語", source: include_str!("ja.tsv"), plural: plural_none, complete_menus: true, catalog: OnceLock::new() },
    LangInfo {
        code: "zh-hans", name: "简体中文", source: include_str!("zh-hans.tsv"), plural: plural_none, complete_menus: true, catalog: OnceLock::new()
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
    LangInfo {
        code: "es",
        name: "Español",
        fluent_source: include_str!("locales/es/messages.ftl"),
        complete_menus: true,
        fluent: OnceLock::new(),
        simple: OnceLock::new(),
    },
    LangInfo {
        code: "ru",
        name: "Русский",
        fluent_source: include_str!("locales/ru/messages.ftl"),
        complete_menus: true,
        fluent: OnceLock::new(),
        simple: OnceLock::new(),
    },
    LangInfo {
        code: "cs",
        name: "Čeština",
        fluent_source: include_str!("locales/cs/messages.ftl"),
        complete_menus: true,
        fluent: OnceLock::new(),
        simple: OnceLock::new(),
    },
    LangInfo { code: "es", name: "Español", source: include_str!("es.tsv"), plural: plural_one_other, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "ru", name: "Русский", source: include_str!("ru.tsv"), plural: plural_russian, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "cs", name: "Čeština", source: include_str!("cs.tsv"), plural: plural_cs, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "fr", name: "Français", source: include_str!("fr.tsv"), plural: plural_fr, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "id", name: "Bahasa Indonesia", source: include_str!("id.tsv"), plural: plural_none, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "ko", name: "한국어", source: include_str!("ko.tsv"), plural: plural_none, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "pl", name: "Polski", source: include_str!("pl.tsv"), plural: plural_polish, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "de", name: "Deutsch", source: include_str!("de.tsv"), plural: plural_one_other, complete_menus: true, catalog: OnceLock::new() },
    // Brazilian Portuguese; `pt`, `pt-BR` and `pt-PT` locales all resolve here (see `candidates`).
    LangInfo {
        code: "pt-br",
        name: "Português (Brasil)",
        source: include_str!("pt-br.tsv"),
        plural: plural_pt,
        complete_menus: true,
        catalog: OnceLock::new(),
    },
    LangInfo { code: "el", name: "Ελληνικά", source: include_str!("el.tsv"), plural: plural_one_other, complete_menus: true, catalog: OnceLock::new() },
    LangInfo { code: "it", name: "Italiano", source: include_str!("it.tsv"), plural: plural_one_other, complete_menus: true, catalog: OnceLock::new() },
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

/// Stable, allocation-free locale identity, independent of replaceable catalog snapshots.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Lang {
    bytes: [u8; config::MAX_CODE_BYTES],
    len: u8,
}

impl std::fmt::Debug for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lang({})", self.code())
    }
}

impl Lang {
    pub const EN: Self = Self { bytes: [b'e', b'n', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], len: 2 };

    fn identity(code: &str) -> Option<Self> {
        if !config::valid_code(code) {
            return None;
        }
        let mut bytes = [0; config::MAX_CODE_BYTES];
        bytes.get_mut(..code.len())?.copy_from_slice(code.as_bytes());
        Some(Self { bytes, len: u8::try_from(code.len()).ok()? })
    }

    pub fn code(&self) -> &str {
        std::str::from_utf8(self.bytes.get(..usize::from(self.len)).unwrap_or(&[])).unwrap_or("en")
    }

    /// Exact registered code, ignoring ASCII case. Locale aliases are resolved by `lang_from_tag`.
    pub fn from_code(code: &str) -> Option<Self> {
        if code.len() > config::MAX_CODE_BYTES {
            return None;
        }
        PACK.with(|pack| {
            if let Some(pack) = pack.borrow().as_ref() {
                return pack.languages.iter().find(|l| l.code.eq_ignore_ascii_case(code)).and_then(|l| Self::identity(&l.code));
            }
            LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code)).and_then(|l| Self::identity(l.code))
        })
    }

    pub fn from_pref(pref: &str) -> Self {
        if pref.eq_ignore_ascii_case("auto") {
            return system_lang();
        }
        Self::from_code(pref).or_else(|| lang_from_tag(pref)).unwrap_or_else(system_lang)
    }

    pub fn all() -> impl Iterator<Item = Self> {
        PACK.with(|pack| match pack.borrow().as_ref() {
            Some(pack) => pack.languages.iter().filter_map(|l| Self::identity(&l.code)).collect::<Vec<_>>(),
            None => LANGUAGES.iter().filter_map(|l| Self::identity(l.code)).collect(),
        })
        .into_iter()
    }

    pub fn name(self) -> Cow<'static, str> {
        PACK.with(|pack| {
            if let Some(language) = pack.borrow().as_ref().and_then(|pack| pack.language(self.code())) {
                return Cow::Owned(language.name.clone());
            }
            Cow::Borrowed(self.bundled().map_or("English", |l| l.name))
        })
    }

    pub fn complete_menus(self) -> bool {
        PACK.with(|pack| {
            pack.borrow()
                .as_ref()
                .and_then(|pack| pack.language(self.code()))
                .map_or_else(|| self.bundled().is_some_and(|l| l.complete_menus), |l| l.complete_menus)
        })
    }
}

/// Most-specific to least-specific tags. Aliases (including Chinese region mappings) are data.
fn candidates(tag: &str) -> Vec<String> {
    if tag.len() > 128 {
        return Vec::new();
    }
    let mut base = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-").to_ascii_lowercase();
    if base.is_empty() || base.split('-').any(str::is_empty) {
        return Vec::new();
    }
    let mut out = Vec::new();
    loop {
        out.push(base.clone());
        let Some(at) = base.rfind('-') else { break };
        base.truncate(at);
    }
    out
}

pub fn lang_from_tag(tag: &str) -> Option<Lang> {
    let candidates = candidates(tag);
    if matches!(candidates.first().map(String::as_str), Some("c" | "posix")) {
        return Some(Lang::EN);
    }
    candidates.iter().find_map(|candidate| {
        Lang::from_code(candidate).or_else(|| {
            PACK.with(|pack| {
                let pack = pack.borrow();
                let languages = pack.as_ref().map_or(&runtime::bundled_manifest().languages, |p| &p.languages);
                languages.iter().find(|l| l.aliases.contains(candidate)).and_then(|l| Lang::identity(&l.code))
            })
        })
    })
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
    static PACK: RefCell<Option<Arc<runtime::LanguagePack>>> = const { RefCell::new(None) };
}

pub fn set_current(lang: Lang) {
    CURRENT.set(lang);
}
pub fn current() -> Lang {
    CURRENT.get()
}
fn set_pack(pack: Option<Arc<runtime::LanguagePack>>) {
    PACK.set(pack);
}

/// Temporarily select a snapshot, restoring it even on unwind; independent threads stay isolated.
pub fn with_pack<R>(pack: Option<Arc<runtime::LanguagePack>>, draw: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<runtime::LanguagePack>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_pack(self.0.take());
        }
    }
    let _restore = Restore(PACK.replace(pack));
    draw()
}

pub fn with_language<R>(lang: Lang, draw: impl FnOnce() -> R) -> R {
    let _restore = language_scope(lang);
    draw()
}

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

pub fn sync_context(ctx: &egui::Context, language: &str) {
    let lang = Lang::from_pref(language);
    set_current(lang);
    let id = egui::Id::new("photocraft-ui-language");
    if ctx.data(|data| data.get_temp::<Lang>(id) != Some(lang)) {
        ctx.data_mut(|data| data.insert_temp(id, lang));
        crate::theme::install_fonts(ctx);
        ctx.request_repaint();
    }
}

/// Does `lang` have a catalog entry for this plain string? (English never does: it is the source.)
pub fn has(lang: Lang, s: &str) -> bool {
    key_id("", s).is_some_and(|id| lang.0.fluent().get_message(id).is_some())
}

pub fn has(lang: Lang, s: &str) -> bool {
    lookup(lang, |c, _| c.plain(s)).is_some()
}
pub fn t(s: &str) -> Cow<'_, str> {
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
    out.push_str(rest);
    out
}
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

    const JA: fn() -> Lang = || Lang::from_code("ja").expect("ja registered");
    const ZH: fn() -> Lang = || Lang::from_code("zh-hant").expect("zh-hant registered");
    const CS: fn() -> Lang = || Lang::from_code("cs").expect("cs registered");
    const ID: fn() -> Lang = || Lang::from_code("id").expect("id registered");

    #[test]
    fn indonesian_tags_resolve() {
        for tag in ["id", "id-ID", "id_ID", "id_ID.UTF-8"] {
            assert_eq!(lang_from_tag(tag), Some(ID()), "{tag}");
        }
        assert_eq!(ID().name(), "Bahasa Indonesia");
    }

    #[test]
    fn locale_resolution_preserves_chinese_script() {
        assert_eq!(lang_from_tag("ja_JP.UTF-8"), Some(language("ja")));
        assert_eq!(lang_from_tag("cs-CZ"), Some(language("cs")));
        assert_eq!(lang_from_tag("en-US"), Some(Lang::EN));
        assert_eq!(lang_from_tag("C"), Some(Lang::EN));
        assert_eq!(lang_from_tag("POSIX"), Some(Lang::EN));
        assert_eq!(lang_from_tag("cs_CZ.UTF-8"), Some(CS()));
        assert_eq!(lang_from_tag("cs-CZ"), Some(CS()));
        assert_eq!(lang_from_tag("fr_FR"), Lang::from_code("fr"));
        assert_eq!(lang_from_tag("de_DE"), Lang::from_code("de"));
        assert_eq!(lang_from_tag("de-AT"), Lang::from_code("de"));
        assert_eq!(lang_from_tag("it_IT.UTF-8"), Lang::from_code("it"));
        assert_eq!(lang_from_tag("it-CH"), Lang::from_code("it"));
        // Traditional Chinese: by region, by script, and with a region after the script.
        assert_eq!(lang_from_tag("zh_TW.UTF-8"), Some(ZH()));
        assert_eq!(lang_from_tag("zh-TW"), Some(ZH()));
        assert_eq!(lang_from_tag("zh-HK"), Some(ZH()));
        assert_eq!(lang_from_tag("zh_MO"), Some(ZH()));
        assert_eq!(lang_from_tag("zh-Hant"), Some(ZH()));
        assert_eq!(lang_from_tag("zh-Hant-TW"), Some(ZH()));
        assert_eq!(lang_from_tag("zh-Hant-HK"), Some(ZH()));
        // Simplified Chinese locales never pick up the Traditional catalog: they resolve to `zh-hans`.
        for tag in ["zh-CN", "zh_CN.UTF-8", "zh_SG", "zh-Hans", "zh-Hans-CN", "zh"] {
            assert_ne!(lang_from_tag(tag), Some(ZH()), "{tag}");
            assert_eq!(lang_from_tag(tag), Lang::from_code("zh-hans"), "{tag}");
        }
        assert_eq!(lang_from_tag(""), None);
        assert_eq!(lang_from_tag("_"), None);
    }

    #[test]
    fn candidates_walk_from_specific_to_general() {
        assert_eq!(candidates("pt_BR.UTF-8"), ["pt-br", "pt"]);
        assert_eq!(candidates("zh_TW"), ["zh-tw", "zh"]);
        assert_eq!(candidates("zh-CN"), ["zh-cn", "zh"]);
        assert_eq!(candidates("zh-Hant-HK"), ["zh-hant-hk", "zh-hant", "zh"]);
    }

    #[test]
    fn preferences_resolve_with_fallback() {
        assert_eq!(Lang::from_pref("ja"), JA());
        assert_eq!(Lang::from_pref("JA"), JA());
        assert_eq!(Lang::from_pref("zh-hant"), ZH());
        assert_eq!(Lang::from_pref("ZH-Hant"), ZH());
        assert_eq!(Lang::from_pref("en"), Lang::EN);
        // `auto` and unknown codes follow the system (English under test).
        assert_eq!(Lang::from_pref("auto"), Lang::EN);
        assert_eq!(Lang::from_pref("xx-unknown"), Lang::EN);
    }

    #[test]
    fn simplified_chinese_covers_dynamic_shortcuts_and_layer_counts() {
        let zh = Lang::from_code("zh-hans").expect("zh-hans registered");
        assert!(zh.complete_menus(), "Simplified Chinese must participate in the coverage gates");
        assert_eq!(Lang::from_pref("ZH-Hans"), zh);
        assert_eq!(tr(zh, "Pixel Layer"), "像素图层");
        assert_eq!(tr(zh, "System Info"), "系统信息");
        for key in ["⌥", "Alt"] {
            assert_eq!(fmt(tr(zh, "Add a mask  (from the selection; {key} inverts)"), &[("key", key)]), format!("添加蒙版  （基于选区；{key} 反相）"));
        }
        for n in [0, 1, 3] {
            assert_eq!(trn(zh, n, "{n} layer", "{n} layers"), format!("{n} 个图层"));
        }
        assert_eq!(tr(zh, "no such label"), "no such label");
    }

    /// Keep the Korean tool/menu vocabulary aligned with the Photoshop equivalents.
    /// Sources and the product-specific vocabulary policy are recorded in `ko.tsv`.
    #[test]
    fn korean_uses_photoshop_terminology() {
        let ko = Lang::from_code("ko").expect("ko registered");
        for (source, expected) in [
            ("Shape", "모양"),
            ("Stroke", "획"),
            ("Smudge Tool", "손가락 도구"),
            ("Eyedropper Tool", "스포이드 도구"),
            ("Rectangular Marquee Tool", "사각형 선택 윤곽 도구"),
            ("Elliptical Marquee Tool", "원형 선택 윤곽 도구"),
            ("Zoom Tool", "돋보기 도구"),
            ("Horizontal Type Tool", "수평 문자 도구"),
            ("Puppet Warp", "퍼펫 뒤틀기"),
            ("Liquify…", "픽셀 유동화…"),
            ("Gaussian Blur…", "가우시안 흐림 효과…"),
            ("Gaussian Blur", "가우시안 흐림 효과"),
            ("Adaptive Wide Angle…", "응용 광각…"),
            ("Render", "렌더"),
            ("Sharpen", "선명 효과"),
            ("Vibrance", "활기"),
            ("Layer Comps", "레이어 구성 요소"),
            ("Vivid Light", "선명한 라이트"),
            ("Hard Mix", "하드 혼합"),
        ] {
            assert_eq!(tr(ko, source), expected, "{source}");
        }
        assert_eq!(tr_id(ko, "filter.sharpen.sharpen", "Sharpen"), "선명하게");
    }

    #[test]
    fn spanish_resolves_and_pluralises() {
        let es = Lang::from_code("es").expect("es registered");
        for tag in ["es", "es_ES.UTF-8", "es-MX", "es-419"] {
            assert_eq!(lang_from_tag(tag), Some(es), "{tag}");
        }
        assert_eq!(first_supported("(\n    \"ja-JP\",\n    \"en-US\"\n)"), Some(language("ja")));
        assert_eq!(Lang::from_pref("unknown"), Lang::EN);
    }

    #[test]
    fn brazilian_portuguese_resolves_and_pluralises() {
        let pt = Lang::from_code("pt-br").expect("pt-br registered");
        for tag in ["pt", "pt-BR", "pt_BR.UTF-8", "pt-PT", "pt_PT.UTF-8"] {
            assert_eq!(lang_from_tag(tag), Some(pt), "{tag}");
        }
        assert_eq!(candidates("pt_PT"), ["pt-pt", "pt"]);
        assert_eq!(PluralRule::Portuguese.index(0), 0);
        assert_eq!(PluralRule::Portuguese.index(u64::MAX), 1);
        assert_eq!(tr(pt, "Layer"), "Camada");
        assert_eq!(trn(pt, 0, "{n} item", "{n} items"), "0 item");
        assert_eq!(trn(pt, 1, "{n} item", "{n} items"), "1 item");
        assert_eq!(trn(pt, 2, "{n} item", "{n} items"), "2 itens");
    }

    #[test]
    fn lookups_fall_back_to_english() {
        assert_eq!(tr(JA(), "no such label"), "no such label");
        assert_eq!(tr(Lang::EN, "Layer"), "Layer");
        assert_eq!(tr(JA(), "Layer"), "レイヤー");
        assert_eq!(tr(ZH(), "Layer"), "圖層");
        assert_eq!(tr(ZH(), "no such label"), "no such label");
        assert_eq!(tr_id(ZH(), "no.such.id", "Layer"), "圖層");
        assert_eq!(tr_id(JA(), "no.such.id", "Layer"), "レイヤー");
        assert_eq!(tr_ctx(JA(), "no such context", "Layer"), "レイヤー");
    }

    #[test]
    fn russian_plural_rules() {
        let ru = || Lang::from_code("ru").expect("ru registered");
        assert_eq!(trn(ru(), 1, "{n} item", "{n} items"), "1 элемент");
        assert_eq!(trn(ru(), 2, "{n} item", "{n} items"), "2 элемента");
        assert_eq!(trn(ru(), 5, "{n} item", "{n} items"), "5 элементов");
        assert_eq!(trn(ru(), 11, "{n} item", "{n} items"), "11 элементов");
        assert_eq!(trn(ru(), 21, "{n} item", "{n} items"), "21 элемент");
        assert_eq!(trn(ru(), 22, "{n} item", "{n} items"), "22 элемента");
        assert_eq!(trn(ru(), 101, "{n} item", "{n} items"), "101 элемент");
        assert_eq!(trn(ru(), 111, "{n} item", "{n} items"), "111 элементов");
    }

    #[test]
    fn catalog_kinds_are_parsed_and_looked_up() {
        let c = Catalog::parse("# c\n\tHello\tこんにちは\n@id\tfile.save\t保存する\nmenu\tWindows\tウィンドウ群\n@plural\t{n} file|{n} files\t{n} 個\n\n");
        assert_eq!(c.plain("Hello"), Some("こんにちは"));
        assert_eq!(c.id("file.save"), Some("保存する"));
        assert_eq!(c.contextual("menu", "Windows"), Some("ウィンドウ群"));
        assert_eq!(c.contextual("other", "Windows"), None);
        assert_eq!(c.plural("{n} file", "{n} files", 0), Some("{n} 個"));
        assert_eq!(c.plural("{n} file", "{n} files", 5), Some("{n} 個"), "an index past the forms clamps");
    }

    #[test]
    fn malformed_lines_are_reported_not_fatal() {
        let (entries, errors) = parse_entries("\tok\tはい\nno tabs here\n\tonly\n\ta\tb\tc\textra\n\t\tempty source\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(errors.len(), 4, "{errors:?}");
        assert_eq!(parse_entries("\ta\\tb\tx\\ny\\\\z\n").0[0], (String::new(), "a\tb".into(), "x\ny\\z".into()));
    }

    #[test]
    fn plurals_and_placeholders() {
        assert_eq!(trn(Lang::EN, 1, "{n} item", "{n} items"), "1 item");
        assert_eq!(trn(Lang::EN, 0, "{n} item", "{n} items"), "0 items");
        assert_eq!(trn(Lang::EN, 7, "{n} item", "{n} items"), "7 items");
        assert_eq!(trn(JA(), 1, "{n} item", "{n} items"), "1 件");
        assert_eq!(trn(JA(), 7, "{n} item", "{n} items"), "7 件");
        assert_eq!(trn(ZH(), 1, "{n} item", "{n} items"), "1 個項目");
        assert_eq!(trn(ZH(), 7, "{n} item", "{n} items"), "7 個項目");
        assert_eq!(trn(CS(), 1, "{n} item", "{n} items"), "1 položka");
        assert_eq!(trn(CS(), 3, "{n} item", "{n} items"), "3 položky");
        assert_eq!(trn(CS(), 5, "{n} item", "{n} items"), "5 položek");
        assert_eq!(trn(CS(), 0, "{n} item", "{n} items"), "0 položek");
        assert_eq!(fmt("{b} before {a}", &[("a", "x"), ("b", "y"), ("c", "z")]), "y before x");
        assert_eq!(fmt("{missing}", &[]), "{missing}");
        assert_eq!(placeholders("a {x} b {y} {"), ["x", "y"]);
    }

    #[test]
    fn french_resolves_and_pluralises() {
        let fr = Lang::from_code("fr").expect("fr registered");
        for tag in ["fr", "fr_FR.UTF-8", "fr-CA", "fr_BE", "fr-CH"] {
            assert_eq!(lang_from_tag(tag), Some(fr), "{tag}");
        }
        assert_eq!(tr(fr, "Layer"), "Calque");
        assert_eq!(tr_id(fr, "select.all", "All"), "Tout sélectionner", "an id override wins over the plain label");
        assert_eq!(tr(fr, "All"), "Tout");
        let forms: Vec<usize> = [0, 1, 2, 5, 100, u64::MAX].into_iter().map(|n| PluralRule::French.index(n)).collect();
        assert_eq!(forms, [0, 0, 1, 1, 1, 1]);
        assert_eq!(trn(fr, 0, "{n} item", "{n} items"), "0 élément");
        assert_eq!(trn(fr, 1, "{n} item", "{n} items"), "1 élément");
        assert_eq!(trn(fr, 3, "{n} item", "{n} items"), "3 éléments");
    }

    #[test]
    fn polish_plural_rule() {
        let forms: Vec<usize> = [0, 1, 2, 4, 5, 12, 14, 21, 22, 25, 112, 122].into_iter().map(plural_polish).collect();
        assert_eq!(forms, [2, 0, 1, 1, 2, 2, 2, 2, 1, 2, 2, 1]);
    }

    #[test]
    fn czech_plural_rule() {
        let forms: Vec<usize> = [0, 1, 2, 3, 4, 5, 11, 12, 21, 22, 100, u64::MAX].into_iter().map(|n| PluralRule::Czech.index(n)).collect();
        assert_eq!(forms, [2, 0, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2]);
        assert_eq!(tr(CS(), "Layer"), "Vrstva");
        assert_eq!(tr_id(CS(), "select.all", "All"), "Vybrat vše", "an id override wins over the plain label");
        assert_eq!(tr(CS(), "All"), "Vše");
    }

    /// Every bundled catalog is well-formed and consistent with its sources.
    #[test]
    fn bundled_catalogs_are_consistent() {
        for l in LANGUAGES {
            assert!(l.code == l.code.to_ascii_lowercase() && !l.name.is_empty(), "{}", l.code);
            let (entries, errors) = parse_entries(l.source);
            assert!(errors.is_empty(), "{}: {errors:?}", l.code);
            let mut seen = std::collections::HashSet::new();
            for (ctx, src, tr) in &entries {
                assert!(seen.insert((ctx.clone(), src.clone())), "{}: duplicate {ctx:?} {src:?}", l.code);
                if ctx == "@plural" {
                    let one_other: Vec<&str> = src.split('|').collect();
                    assert_eq!(one_other.len(), 2, "{}: plural source must be `one|other`: {src:?}", l.code);
                    let forms = (0..=1000).map(|n| l.plural.index(n)).max().unwrap_or(0) + 1;
                    assert_eq!(tr.split('|').count(), forms, "{}: {forms} plural forms expected in {src:?}", l.code);
                    for form in tr.split('|') {
                        let mut want = placeholders(one_other[1]);
                        let mut got = placeholders(form);
                        want.sort_unstable();
                        got.sort_unstable();
                        assert_eq!(want, got, "{}: placeholders differ in {src:?}", l.code);
                    }
                    continue;
                }
                let mut want = placeholders(src);
                let mut got = placeholders(tr);
                want.sort_unstable();
                got.sort_unstable();
                assert_eq!(want, got, "{}: placeholders differ in {src:?}", l.code);
                if ctx.is_empty() {
                    assert_eq!(src.ends_with('…'), tr.ends_with('…'), "{}: ellipsis mismatch: {src:?}", l.code);
                }
                if ctx == "@id" {
                    assert!(crate::menus::is_live(src) || crate::menu_catalog::CATALOG.iter().any(|m| m.3 == src), "{}: unknown command id {src:?}", l.code);
                }
            }
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
        assert!(literals.len() > 300, "scan found only {} literals", literals.len());
        for l in LANGUAGES.iter().filter(|l| l.complete_menus) {
            let cat = l.catalog();
            let missing: Vec<_> = literals.iter().filter(|s| !KEEP_AS_IS.contains(&s.as_str()) && cat.plain(s).is_none()).collect();
            assert!(missing.is_empty(), "{}: untranslated tl! strings: {missing:#?}", l.code);
        }
    }

    /// Section names are dynamic labels, so the literal scanner cannot cover them.
    #[test]
    fn brush_section_names_are_translated() {
        for lang in Lang::all().filter(|l| l.complete_menus()) {
            for (name, _) in crate::brush_panel::SECTIONS {
                assert!(has(lang, name), "{} missing brush section: {name}", lang.code());
            }
        }
    }

    #[test]
    fn layer_color_names_are_translated() {
        for lang in Lang::all().filter(|l| l.complete_menus()) {
            for color in photocraft_doc::LabelColor::ALL {
                let cat = lang.0.catalog();
                assert!(
                    cat.contextual("layerLabel", color.label()).or_else(|| cat.plain(color.label())).is_some(),
                    "{} missing {}",
                    lang.code(),
                    color.label()
                );
            }
        }
        let de = lang_from_tag("de").unwrap();
        assert_eq!(tr_ctx(de, "layerLabel", "No Color"), "Keine Farbe");
        assert_eq!(tr_ctx(de, "layerLabel", "Seafoam"), "Meeresschaum");
        assert_eq!(tr(Lang::EN, "Seafoam"), "Seafoam");
    }

    /// Font style labels are built from dynamic words (weights, "Italic"), so the literal
    /// scanner cannot cover them; "Light" there is a weight, distinct from the Camera Raw
    /// "Light" section (`type_tool::style_label`).
    #[test]
    fn font_weight_names_are_translated() {
        const TERMS: &[&str] = &["Thin", "ExtraLight", "Light", "Regular", "Medium", "SemiBold", "Bold", "ExtraBold", "Black", "Italic"];
        for lang in Lang::all().filter(|l| l.complete_menus()) {
            for t in TERMS {
                assert!(lang.0.catalog().contextual("fontWeight", t).is_some(), "{} missing font weight: {t}", lang.code());
            }
            assert_ne!(tr_ctx(lang, "cameraRaw", "Light"), tr_ctx(lang, "fontWeight", "Light"), "{}: Camera Raw Light vs the font weight", lang.code());
        }
    }

    /// Blend mode names come from the colour crate; each must be translated.
    #[test]
    fn blend_mode_names_are_translated() {
        for l in LANGUAGES.iter().filter(|l| l.complete_menus) {
            for m in std::iter::once(photocraft_color::BlendMode::PassThrough).chain(photocraft_color::BlendMode::LAYER_MODES) {
                assert!(l.catalog().plain(m.label()).is_some(), "{}: blend mode {:?}", l.code, m.label());
            }
        }
    }

    // Dynamic dialog labels generated by label_of() are not seen by the tl! scanner.
    #[test]
    fn color_lookup_export_scope_labels_are_translated() {
        for lang in Lang::all().filter(|l| *l != Lang::EN) {
            for source in ["Scope", "Selected"] {
                assert!(lang.catalog().plain(source).is_some(), "{}: missing LUT export label {source:?}", lang.code());
                assert_ne!(tr(lang, source), source, "{}: untranslated LUT export label {source:?}", lang.code());
            }
        }
    }

    #[test]
    fn mixer_brush_ui_strings_have_translations_in_every_registered_language() {
        const STRINGS: &[&str] = &["Mixer Brush", "Mixer Brush Tool", "Wet", "Load", "Mix", "Flow", "Sample All Layers"];
        for lang in Lang::all() {
            for source in STRINGS {
                let translated = tr(lang, source);
                if lang == Lang::EN {
                    assert_eq!(translated, *source, "English source string {source}");
                } else {
                    assert_ne!(translated, *source, "{} is missing {source:?}", lang.code());
                }
            }
        }
    }

    #[test]
    fn pattern_stamp_ui_strings_have_translations_in_every_registered_language() {
        const STRINGS: &[&str] = &["Pattern Stamp Tool", "Pattern Stamp", "Impressionist", "Aligned"];
        for lang in Lang::all() {
            for source in STRINGS {
                let translated = tr(lang, source);
                if lang == Lang::EN {
                    assert_eq!(translated, *source, "English source string {source}");
                } else {
                    assert_ne!(translated, *source, "{} is missing {source:?}", lang.code());
                }
            }
        }
    }

    /// Camera Raw includes dynamic colour-band labels and contextual labels that the generic
    /// tl! scanner cannot see. Cover the partial catalog too, without claiming whole-app coverage.
    #[test]
    fn camera_raw_labels_are_translated_in_every_available_language() {
        let sources = [include_str!("../camera_raw_ui.rs"), include_str!("../camera_raw_scope_ui.rs")];
        let mut labels = std::collections::BTreeSet::new();
        for source in sources {
            let code = source.split("#[cfg(test)]").next().unwrap();
            for marker in ["tl!(\"", "row(ui, &mut dirty, \"", "row(ui, dirty, \"", "section(ui, \"", "wheel(ui, &mut dirty, \"", "=> \""] {
                for tail in code.split(marker).skip(1) {
                    labels.insert(tail.split('"').next().unwrap());
                }
            }
        }
        let bands = sources[0].split("const BANDS:").nth(1).unwrap().split(" = ").nth(1).unwrap().split(';').next().unwrap();
        for band in bands.split('"').skip(1).step_by(2) {
            labels.insert(band);
        }
        assert!(labels.len() >= 66, "missing Camera Raw source labels: {labels:?}");
        for lang in Lang::all().filter(|l| *l != Lang::EN) {
            let catalog = lang.bundled().expect("bundled language").catalog();
            let missing: Vec<_> = labels.iter().filter(|s| catalog.contextual("cameraRaw", s).or_else(|| catalog.plain(s)).is_none()).collect();
            assert!(missing.is_empty(), "{}: Camera Raw labels: {missing:?}", lang.code());
            assert_ne!(tr_ctx(lang, "cameraRaw", "Highlights"), tr_ctx(lang, "cameraRaw", "Lights"), "{}: distinct curve regions", lang.code());
            assert_ne!(tr_ctx(lang, "cameraRaw", "Shadows"), tr_ctx(lang, "cameraRaw", "Darks"), "{}: distinct curve regions", lang.code());
        }
        let ru = Lang::from_code("ru").unwrap();
        assert_eq!(tr_ctx(ru, "cameraRaw", "Vibrance"), "Красочность");
        assert_eq!(tr_ctx(ru, "cameraRaw", "Aqua"), "Голубые");
        let text = fmt(tr(ru, "Camera Raw Filter ({layer})"), &[("layer", "{Background} 影像")]);
        assert_eq!(text, "Фильтр Camera Raw ({Background} 影像)", "user layer names are not translated");
    }
}

#[cfg(test)]
mod live_tests;

#[cfg(test)]
mod pack_tests;
