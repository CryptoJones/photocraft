# PhotoCraft language resources

Edit `manifest.json` to register a language, its native name, locale aliases, plural rule,
fallback and TSV resource. `manifest.schema.json` supplies editor completion. Rust registry
code is generated during the build; translators do not need to edit it.

Each UTF-8 TSV row is `context<TAB>English source<TAB>translation`. An empty context is a
plain key; `@id` disambiguates command labels; `@plural` contains pipe-separated forms.
Preserve placeholders, escapes and the final dialog ellipsis. The header in `ja.tsv` gives
examples. Translation licensing and provenance are in `LICENSE-translations.txt` and
[ATTRIBUTION.md](../../../ATTRIBUTION.md).

From the workspace root:

```sh
cargo xtask i18n --check
cargo xtask i18n --report --json
cargo xtask i18n --init nl --name Nederlands --plural-rule one_other
```

New catalogs start with commented TODO rows and `completeMenus: false`. Uncomment a TODO
row, leave its context empty and translate it. Enable complete coverage once the missing-key
report is empty. Add translator attribution in the same change.

The same format works for external desktop language packs. Use the config directory's
`Locales` folder or `PHOTOCRAFT_LOCALES_DIR`; valid file edits are hot-reloaded. Full details,
limits, fallback rules, error recovery and agent commands are in
[localization.md](../../../docs/localization.md).
