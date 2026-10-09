//! Validated, bounded and atomically replaceable language-pack snapshots.

use super::{
    LANGUAGES,
    catalog::Catalog,
    config::{self, LanguageConfig, Manifest},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

pub fn bundled_manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        Manifest::parse(include_str!("../../locales/manifest.json")).unwrap_or_else(|_| Manifest {
            schema: None,
            schema_version: 1,
            fallback_locale: "en".into(),
            reload_interval_ms: 1000,
            languages: LANGUAGES
                .iter()
                .map(|l| LanguageConfig {
                    code: l.code.into(),
                    name: l.name.into(),
                    catalog: None,
                    plural_rule: l.plural,
                    fallback: None,
                    aliases: Vec::new(),
                    complete_menus: l.complete_menus,
                })
                .collect(),
        })
    })
}

/// One snapshot owns all external strings. Replacing it frees previous catalogs after readers
/// finish; no strings are leaked to manufacture static lifetimes.
#[derive(Debug)]
pub struct LanguagePack {
    pub manifest: Manifest,
    pub languages: Vec<LanguageConfig>,
    pub(super) catalogs: BTreeMap<String, Catalog>,
}

impl LanguagePack {
    pub fn build(manifest: Manifest, files: &BTreeMap<String, String>) -> Result<Self, String> {
        manifest.validate_fields()?;
        let mut total = 0usize;
        if files.len() > config::MAX_LANGUAGES {
            return Err("too many catalog files".into());
        }
        for (name, text) in files {
            if name.len() > 80 {
                return Err("catalog file names must be at most 80 bytes".into());
            }
            if !config::valid_catalog_name(name) || text.len() > config::MAX_CATALOG_BYTES {
                return Err(format!("{name}: invalid file name or catalog exceeds 2 MiB"));
            }
            total = total.checked_add(text.len()).ok_or("language pack size overflow")?;
            if total > config::MAX_PACK_BYTES {
                return Err("language pack exceeds 16 MiB".into());
            }
            if !manifest.languages.iter().any(|language| language.catalog.as_ref() == Some(name)) {
                return Err(format!("unused catalog {name:?}"));
            }
        }
        let mut languages = bundled_manifest().languages.clone();
        let mut catalogs = BTreeMap::new();
        for language in &manifest.languages {
            if let Some(file) = &language.catalog {
                let text = files.get(file).ok_or_else(|| format!("{}: missing catalog {file}", language.code))?;
                catalogs.insert(language.code.clone(), Catalog::parse_checked(text, language.plural_rule).map_err(|e| format!("{file}: {e}"))?);
            } else if !languages.iter().any(|existing| existing.code == language.code) {
                return Err(format!("{}: a new language requires a catalog file", language.code));
            }
            if let Some(existing) = languages.iter_mut().find(|existing| existing.code == language.code) {
                *existing = language.clone();
            } else {
                languages.push(language.clone());
            }
        }
        manifest.validate_registry(&languages)?;
        Ok(Self { manifest, languages, catalogs })
    }

    pub fn language(&self, code: &str) -> Option<&LanguageConfig> {
        self.languages.iter().find(|l| l.code == code)
    }

    /// Deserialize control-channel data by reference, with bounds before allocating strings.
    pub fn from_params(params: &serde_json::Value) -> Result<Self, String> {
        use serde::Deserialize;
        let object = params.as_object().ok_or("language-pack params must be an object")?;
        if object.keys().any(|key| key != "manifest" && key != "catalogs") {
            return Err("expected only manifest and catalogs".into());
        }
        let value = object.get("manifest").ok_or("missing manifest")?;
        if value.as_object().is_none_or(|object| object.len() > 5 || object.keys().any(|key| key.len() > 64)) {
            return Err("manifest must be a bounded object".into());
        }
        for key in ["$schema", "fallbackLocale"] {
            if value.get(key).and_then(serde_json::Value::as_str).is_some_and(|s| s.len() > 256) {
                return Err(format!("{key} is too long"));
            }
        }
        let languages = value.get("languages").and_then(serde_json::Value::as_array).ok_or("manifest.languages must be an array")?;
        if languages.len() > config::MAX_LANGUAGES {
            return Err("too many languages".into());
        }
        for language in languages {
            if language.as_object().is_none_or(|object| object.len() > 7 || object.keys().any(|key| key.len() > 64)) {
                return Err("language must be a bounded object".into());
            }
            if language.get("aliases").and_then(serde_json::Value::as_array).is_some_and(|a| a.len() > 32) {
                return Err("too many aliases".into());
            }
            if language
                .get("aliases")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|a| a.iter().any(|v| v.as_str().is_none_or(|s| s.len() > config::MAX_CODE_BYTES)))
            {
                return Err("invalid or oversized alias".into());
            }
            for key in ["code", "name", "catalog", "fallback"] {
                if language.get(key).and_then(serde_json::Value::as_str).is_some_and(|s| s.len() > 128) {
                    return Err(format!("manifest {key} is too long"));
                }
            }
        }
        let manifest = Manifest::deserialize(value).map_err(|e| format!("manifest: {e}"))?;
        let files = object.get("catalogs").and_then(serde_json::Value::as_object).ok_or("catalogs must be an object of file names and TSV text")?;
        if files.len() > config::MAX_LANGUAGES {
            return Err("too many catalogs".into());
        }
        let mut total = 0usize;
        let mut texts = BTreeMap::new();
        for (name, value) in files {
            if name.len() > 80 {
                return Err("catalog file names must be at most 80 bytes".into());
            }
            let text = value.as_str().ok_or_else(|| format!("{name}: catalog must be UTF-8 text"))?;
            total = total.checked_add(text.len()).ok_or("language pack size overflow")?;
            if text.len() > config::MAX_CATALOG_BYTES || total > config::MAX_PACK_BYTES || !config::valid_catalog_name(name) {
                return Err(format!("{name}: invalid or oversized catalog"));
            }
            texts.insert(name.clone(), text.to_string());
        }
        Self::build(manifest, &texts)
    }
}

/// Platform implementations deliver only fully validated packs. None restores the bundle.
pub trait LocaleSource {
    fn poll(&mut self) -> Option<Result<Option<Arc<LanguagePack>>, String>>;
    fn reload(&mut self) -> Result<(), String>;
}

#[derive(Default)]
pub struct Runtime {
    pub pack: Option<Arc<LanguagePack>>,
    pub generation: u64,
    pub last_error: Option<String>,
}

impl Runtime {
    pub fn status(&self) -> crate::state::LocalizationStatus {
        crate::state::LocalizationStatus {
            generation: self.generation,
            languages: self
                .pack
                .as_ref()
                .map_or_else(|| LANGUAGES.iter().map(|l| l.code.to_string()).collect(), |p| p.languages.iter().map(|l| l.code.clone()).collect()),
            error: self.last_error.clone(),
        }
    }
    pub fn activate(&self) {
        super::set_pack(self.pack.clone());
    }

    pub fn install(&mut self, pack: Option<Arc<LanguagePack>>) {
        self.pack = pack;
        self.generation = self.generation.saturating_add(1);
        self.last_error = None;
        self.activate();
    }

    pub fn tick(&mut self, source: Option<&mut Box<dyn LocaleSource>>) -> bool {
        if let Some(update) = source.and_then(|source| source.poll()) {
            match update {
                Ok(pack) => {
                    self.install(pack);
                    return true;
                }
                Err(error) => {
                    self.last_error = Some(error);
                    self.activate();
                    return true;
                }
            }
        }
        self.activate();
        false
    }
}

pub fn invoke(app: &mut crate::PhotocraftApp, ctx: &egui::Context, id: &str, params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    Some(match id {
        "ui.i18n.reload" => {
            if !params.as_object().is_some_and(|object| object.is_empty()) {
                return Some(Err("reload expects an empty object".into()));
            }
            app.services
                .locales
                .as_mut()
                .ok_or_else(|| "no external language source is configured".to_string())
                .and_then(|source| source.reload())
                .map(|()| serde_json::json!({"pending": true}))
        }
        "ui.i18n.load" => LanguagePack::from_params(params).map(|pack| {
            app.localizations.install(Some(Arc::new(pack)));
            app.ui.localizations = app.localizations.status();
            ctx.request_repaint();
            serde_json::json!({"generation": app.localizations.generation, "languages": app.ui.localizations.languages})
        }),
        _ => return None,
    })
}
