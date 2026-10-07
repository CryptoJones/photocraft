//! Validate configuration contracts, publication boundaries and real filesystem reloads.
use super::*;
use runtime::{LanguagePack, LocaleSource, Runtime};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc};

fn params(code: &str, text: &str) -> Value {
    json!({"manifest":{"schemaVersion":1,"reloadIntervalMs":250,"languages":[{
        "code":code,"name":"Test language","catalog":"test.tsv","pluralRule":"one_other","fallback":"en","aliases":["xx-aa"]
    }]},"catalogs":{"test.tsv":text}})
}
fn pack(code: &str, text: &str) -> Arc<LanguagePack> {
    Arc::new(LanguagePack::from_params(&params(code, text)).expect("valid pack"))
}

#[test]
fn overlay_updates_strings_metadata_aliases_and_new_locales() {
    with_pack(
        Some(pack("nl", "\tLayer\tLaag\nmenu\tLayer\tLagenmenu\n@id\tlayer.delete\tVerwijderen\n@plural\t{n} item|{n} items\t{n} ding|{n} dingen\n")),
        || {
            let de = Lang::from_code("nl").expect("new locale");
            assert_eq!(de.name(), "Test language");
            assert_eq!(lang_from_tag("NL-nl"), Some(de));
            assert_eq!(lang_from_tag("xx-AA"), Some(de));
            assert_eq!(tr(de, "Layer"), "Laag");
            assert_eq!(tr_ctx(de, "menu", "Layer"), "Lagenmenu");
            assert_eq!(tr_id(de, "layer.delete", "Delete Layer"), "Verwijderen");
            assert_eq!(trn(de, 2, "{n} item", "{n} items"), "2 dingen");
            assert_eq!(tr(de, "untranslated"), "untranslated");
        },
    );
    assert!(Lang::from_code("nl").is_none());
}

#[test]
fn fallback_uses_each_locales_plural_rule_and_preserves_bundled_keys() {
    let value = json!({"manifest":{"schemaVersion":1,"languages":[
        {"code":"xx","name":"Example","catalog":"xx.tsv","pluralRule":"russian","fallback":"fr"},
        {"code":"fr","name":"Français modifié","catalog":"fr.tsv","pluralRule":"french","fallback":"en"}
    ]},"catalogs":{"xx.tsv":"\tOnly here\tCustom\n","fr.tsv":"\tLayer\tUpdated\n"}});
    with_pack(Some(Arc::new(LanguagePack::from_params(&value).unwrap())), || {
        let xx = Lang::from_code("xx").unwrap();
        let fr = Lang::from_code("fr").unwrap();
        assert_eq!(tr(xx, "Layer"), "Updated");
        assert_eq!(tr(fr, "File"), "Fichier");
        assert_eq!(trn(xx, 0, "{n} item", "{n} items"), "0 élément");
        assert_eq!(trn(xx, 2, "{n} item", "{n} items"), "2 éléments");
    });
}

#[test]
fn new_upstream_languages_keep_aliases_auto_and_bundled_fallbacks_during_reload() {
    for (code, tag, name, rule, aliases, layer) in [
        ("de", "de-AT", "Deutsch", "one_other", vec![], "Ebene"),
        ("pt-br", "pt-PT", "Português (Brasil)", "portuguese", vec!["pt", "pt-pt"], "Camada"),
        ("it", "it-IT", "Italiano", "one_other", vec![], "Livello"),
    ] {
        let locale = Lang::from_code(code).expect("upstream language retained");
        assert_eq!(locale.name(), name);
        system::with_system_tags(&[tag], || {
            assert_eq!(Lang::from_pref("auto"), locale);
            for text in ["Pack A", "Pack B"] {
                let value = json!({"manifest":{"schemaVersion":1,"languages":[{
                    "code":code,"name":name,"catalog":"overlay.tsv","pluralRule":rule,"aliases":aliases
                }]},"catalogs":{"overlay.tsv":format!("\tFile\t{text}\n")}});
                with_pack(Some(Arc::new(LanguagePack::from_params(&value).expect("overlay"))), || {
                    assert_eq!(Lang::from_pref("auto"), locale);
                    assert_eq!(tr(locale, "File"), text);
                    assert_eq!(tr(locale, "Layer"), layer);
                });
            }
            assert_eq!(Lang::from_pref("auto"), locale);
            assert_eq!(locale.name(), name);
        });
    }
}

#[test]
fn configurations_reject_unknown_fields_conflicting_aliases_and_fallback_cycles() {
    for (pointer, value) in [
        ("/manifest/schemaVersion", json!(2)),
        ("/manifest/reloadIntervalMs", json!(0)),
        ("/manifest/languages/0/code", json!("../../de")),
        ("/manifest/languages/0/code", json!("auto")),
        ("/manifest/languages/0/aliases", json!(["posix"])),
        ("/manifest/languages/0/catalog", json!("../test.tsv")),
        ("/manifest/languages/0/fallback", json!("de")),
        ("/manifest/languages/0/fallback", json!("zz")),
        ("/manifest/languages/0/aliases", json!(["fr"])),
        ("/manifest/languages/0/aliases", json!(["xx-aa", "xx-aa"])),
        ("/manifest/languages/0/pluralRule", json!("unknown")),
        ("/manifest/languages/0/name", json!("bad\nname")),
    ] {
        let mut value_params = params("de", "\tLayer\tEbene\n");
        *value_params.pointer_mut(pointer).unwrap() = value;
        assert!(LanguagePack::from_params(&value_params).is_err(), "{pointer}");
    }
    let mut unknown = params("de", "");
    unknown["manifest"]["futureOption"] = json!(true);
    assert!(LanguagePack::from_params(&unknown).is_err());
    let mut ambiguous = params("de", "");
    ambiguous["manifest"]["languages"].as_array_mut().unwrap().push(json!({
        "code":"xx","name":"Example","pluralRule":"none","catalog":"other.tsv","fallback":"de","aliases":["xx-aa"]
    }));
    ambiguous["catalogs"]["other.tsv"] = json!("");
    assert!(LanguagePack::from_params(&ambiguous).is_err());
}

#[test]
fn strict_catalog_updates_reject_bad_rows_duplicates_placeholders_and_plural_forms() {
    // Windows editors may write a BOM before the first data row, without a comment header.
    assert_eq!(Catalog::parse("\u{feff}\tLayer\tEbene\n").plain("Layer"), Some("Ebene"));
    for text in [
        "broken line",
        "\tLayer\tA\n\tLayer\tB\n",
        "\tVersion {version}\tVersion\n",
        "\tSave…\tSave\n",
        "@plural\t{n} item|{n} items\t{n} item\n",
        "@plural\tone|other\tone|\n",
    ] {
        assert!(LanguagePack::from_params(&params("de", text)).is_err(), "{text}");
    }
    // Reordered parameters stay valid; values are inserted once, never recursively expanded.
    let pack = pack("de", "\t{a} before {b}\t{b} vor {a}\n");
    with_pack(Some(pack), || assert_eq!(fmt(tr(Lang::from_code("de").unwrap(), "{a} before {b}"), &[("a", "{b}"), ("b", "B")]), "B vor {b}"));
}

#[test]
fn hostile_control_payloads_fail_gracefully_before_copying_oversized_catalogs() {
    for value in [Value::Null, json!([]), json!({}), json!({"manifest":false,"catalogs":{}}), json!({"manifest":{"languages":false},"catalogs":{}})] {
        assert!(LanguagePack::from_params(&value).is_err());
    }
    let mut large = params("de", "");
    large["catalogs"]["test.tsv"] = json!("a".repeat(config::MAX_CATALOG_BYTES + 1));
    assert!(LanguagePack::from_params(&large).is_err());
    let mut large = params("de", "");
    large["manifest"]["languages"][0]["aliases"] = json!(["a".repeat(100_000)]);
    assert!(LanguagePack::from_params(&large).is_err());
    assert!(Lang::from_code(&"a".repeat(100_000)).is_none());
    assert!(lang_from_tag(&"a".repeat(100_000)).is_none());
    let mut large = params("de", "");
    large["catalogs"].as_object_mut().unwrap().insert("x".repeat(100_000), Value::Null);
    assert!(LanguagePack::from_params(&large).unwrap_err().len() < 256, "oversized keys produce bounded diagnostics");
}

struct Queue(VecDeque<Result<Option<Arc<LanguagePack>>, String>>);
impl LocaleSource for Queue {
    fn poll(&mut self) -> Option<Result<Option<Arc<LanguagePack>>, String>> {
        self.0.pop_front()
    }
    fn reload(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn invalid_updates_keep_last_good_snapshot_and_valid_updates_recover() {
    with_pack(None, || {
        let mut runtime = Runtime::default();
        let first = pack("fr", "\tLayer\tFirst\n");
        let weak = Arc::downgrade(&first);
        let mut source: Box<dyn LocaleSource> =
            Box::new(Queue(VecDeque::from([Ok(Some(first)), Err("bad file".into()), Ok(Some(pack("fr", "\tLayer\tSecond\n"))), Ok(None)])));
        runtime.tick(Some(&mut source));
        let fr = Lang::from_code("fr").unwrap();
        let retained = tr(fr, "Layer");
        assert_eq!(retained, "First");
        runtime.tick(Some(&mut source));
        assert_eq!(tr(fr, "Layer"), "First");
        assert_eq!(runtime.generation, 1);
        assert_eq!(runtime.status().error.as_deref(), Some("bad file"));
        runtime.tick(Some(&mut source));
        assert_eq!(tr(fr, "Layer"), "Second");
        assert!(runtime.last_error.is_none());
        assert!(weak.upgrade().is_none(), "previous snapshot is freed, retained display text owns its value");
        assert_eq!(retained, "First");
        runtime.tick(Some(&mut source));
        assert_eq!(tr(fr, "Layer"), "Calque");
        assert_eq!(runtime.generation, 3);
    });
}

#[test]
fn snapshots_are_thread_local_and_restore_on_unwind() {
    with_pack(Some(pack("fr", "\tLayer\tParent\n")), || {
        let fr = Lang::from_code("fr").unwrap();
        std::thread::spawn(move || assert_eq!(tr(fr, "Layer"), "Calque")).join().unwrap();
        let result = std::panic::catch_unwind(|| with_pack(Some(pack("fr", "\tLayer\tChild\n")), || panic!("restore snapshot")));
        assert!(result.is_err());
        assert_eq!(tr(fr, "Layer"), "Parent");
    });
}

#[test]
fn ui_commands_publish_agent_readable_status_and_reject_invalid_params() {
    with_pack(None, || {
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        crate::menus::invoke(&mut app, &ctx, "ui.i18n.load", params("de", "\tLayer\tEbene\n")).unwrap();
        assert_eq!(app.ui.localizations.generation, 1);
        assert!(app.ui.localizations.languages.contains(&"de".into()));
        assert_eq!(tr(Lang::from_code("de").unwrap(), "Layer"), "Ebene");
        assert!(crate::menus::invoke(&mut app, &ctx, "ui.i18n.load", params("de", "broken")).is_err());
        assert_eq!(app.ui.localizations.generation, 1);
        assert!(crate::menus::invoke(&mut app, &ctx, "ui.i18n.reload", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "ui.i18n.reload", json!({"unexpected":true})).is_err());
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_watcher_handles_edits_errors_manifest_removal_and_manual_reload() {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("test.tsv"));
            let _ = std::fs::remove_file(self.0.join("manifest.json"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let dir = Temp(std::env::temp_dir().join(format!("photocraft-locales-test-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let params = params("de", "\tLayer\tFirst\n");
    std::fs::write(dir.0.join("manifest.json"), serde_json::to_string(&params["manifest"]).unwrap()).unwrap();
    std::fs::write(dir.0.join("test.tsv"), "\tLayer\tFirst\n").unwrap();
    let mut watcher = native::watch(dir.0.clone(), egui::Context::default()).unwrap();
    let next = |watcher: &mut native::Watcher| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(update) = watcher.poll() {
                return update;
            }
            assert!(Instant::now() < deadline, "watcher deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    assert!(next(&mut watcher).unwrap().is_some());
    // Same-length edit: detection compares bytes rather than only metadata.
    std::fs::write(dir.0.join("test.tsv"), "\tLayer\tOther\n").unwrap();
    let updated = next(&mut watcher).unwrap().unwrap();
    with_pack(Some(updated), || assert_eq!(tr(Lang::from_code("de").unwrap(), "Layer"), "Other"));
    std::fs::write(dir.0.join("test.tsv"), "invalid TSV").unwrap();
    assert!(next(&mut watcher).is_err());
    std::fs::write(dir.0.join("test.tsv"), "\tLayer\tAgain\n").unwrap();
    assert!(next(&mut watcher).unwrap().is_some());
    watcher.reload().unwrap();
    assert!(next(&mut watcher).unwrap().is_some());
    std::fs::remove_file(dir.0.join("manifest.json")).unwrap();
    assert!(next(&mut watcher).unwrap().is_none());
}

#[test]
fn loaded_translation_edits_reach_rendered_menus_and_preserve_the_document() {
    with_pack(None, || {
        with_language(Lang::EN, || {
            let mut h = super::live_tests::harness();
            let document = h.state().session.active().unwrap().doc.clone();
            h.state_mut().run("prefs.set", json!({"path":"interface.language","value":"fr"})).unwrap();
            let ctx = h.ctx.clone();
            for word in ["Fichier chargé", "Fichier rechargé"] {
                crate::menus::invoke(h.state_mut(), &ctx, "ui.i18n.load", params("fr", &format!("\tFile\t{word}\n"))).unwrap();
                h.step();
                assert!(super::live_tests::drawn_text(&h).iter().any(|s| s == word));
                assert!(Arc::ptr_eq(&document, &h.state().session.active().unwrap().doc));
            }
            assert!(crate::menus::invoke(h.state_mut(), &ctx, "ui.i18n.load", params("fr", "broken")).is_err());
            h.step();
            assert!(super::live_tests::drawn_text(&h).iter().any(|s| s == "Fichier rechargé"));
            assert_eq!(crate::control::inspect(h.state(), &ctx)["localizations"]["generation"], 2);
            h.state_mut().services.locales = Some(Box::new(Queue(VecDeque::from([Err("invalid translation update".into())]))));
            h.step();
            assert_eq!(h.state().ui.localizations.error.as_deref(), Some("invalid translation update"));
            h.step();
            assert_eq!(h.state().ui.localizations.error.as_deref(), Some("invalid translation update"));
        })
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn bundled_coverage_report_and_new_language_scaffold_are_usable() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let report = maintenance::report(&dir).unwrap();
    for language in report["languages"].as_array().unwrap() {
        assert!(language["missing"].as_array().unwrap().is_empty(), "{}: {}", language["code"], language["missing"]);
    }
    let dir = std::env::temp_dir().join(format!("photocraft-scaffold-test-{}", std::process::id()));
    maintenance::scaffold(&dir, "nl", "Nederlands", PluralRule::OneOther, |path, text| {
        photocraft_format::atomic_write(path, text.as_bytes()).map_err(|e| e.to_string())
    })
    .unwrap();
    let inputs = native::read_inputs(&dir).unwrap().unwrap();
    assert!(LanguagePack::build(inputs.manifest, &inputs.files).is_ok());
    assert!(maintenance::scaffold(&dir, "nl", "Nederlands", PluralRule::OneOther, |_, _| Ok(())).is_err());
    assert_eq!(std::fs::read_to_string(dir.join("nl.tsv")).unwrap().lines().filter(|l| !l.starts_with('#')).count(), 0);
    std::fs::remove_file(dir.join("nl.tsv")).unwrap();
    std::fs::remove_file(dir.join("manifest.json")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
