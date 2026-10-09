# Localisation, language packs and live updates

PhotoCraft uses one Rust i18n implementation for the native egui UI and its WebAssembly build.
English source keys and command IDs stay stable. Translation resources are UTF-8 files,
independent of the lookup code.

## Shipped languages

| Language | Preference code | Integer plural forms |
|---|---|---|
| English | `en` | one / other |
| 简体中文 | `zh-hans` | one form |
| 日本語 | `ja` | one form |
| 한국어 | `ko` | one form |
| Русский | `ru` | one / few / many |
| Français | `fr` | singular for 0 and 1 / plural for 2+ |
| 繁體中文 | `zh-hant` | one form |
| Español | `es` | one / other |
| Čeština | `cs` | one / few / other |
| Deutsch | `de` | one / other |
| Bahasa Indonesia | `id` | one form |
| Português (Brasil) | `pt-br` | singular for 0 and 1 / plural for 2+ |
| Italiano | `it` | one / other |
| Polski | `pl` | one / few (2–4, not 12–14) / many |
| Ελληνικά | `el` | one / other |

All twelve non-English catalogs cover the enforced menu, widget, blend-mode, brush-section
and generated preference keys. Coverage measures the presence and structural validity of
translations; native-speaker review is still needed for wording. Engine errors and some
status messages remain English. User document, layer and preset names keep their original text.

## Switch languages

Open **Edit > Preferences > Interface > Language**. The dialog previews the selected language;
**Cancel** discards it, **Apply** commits it while keeping the dialog open, and **OK** commits
and closes. The main window follows the committed preference on its next draw, without
restarting or changing documents, undo history or tools. Preferences persist across launches.

The existing preference store saves the selection for future launches. `auto` follows the
native system locale; an unsupported code follows the same fallback. Regional tags such as
`fr-CA`, `ko-KR` and `zh-CN` resolve to the corresponding registered catalog. Traditional and
Simplified Chinese remain distinct. Every Portuguese locale (`pt`, `pt-BR`, `pt-PT`) uses the
Brazilian Portuguese catalog through manifest aliases.

### First launch and system language

With no saved preference, `interface.language` defaults to `auto`, so the first rendered
window uses the first supported system UI language. An older preferences file without this
field also uses Auto. If detection fails or none of the preferred languages is registered,
the UI uses English (`en`). A saved manual language choice takes precedence on later launches;
startup never overwrites it or writes the detected language into the preference file.

The native-only, pinned `sys-locale` dependency provides safe Rust access to the platform APIs:

- **Windows:** `GetUserPreferredUILanguages`, in the user's preferred order. This uses UI
  languages rather than the Region setting for dates and numbers; they can differ.
- **macOS:** `CFLocaleCopyPreferredLanguages`, in preferred order, without starting `defaults`
  or depending on a particular `.plist` representation.
- **Linux/BSD:** the platform's standard locale environment, including `LANGUAGE` lists.

`PHOTOCRAFT_LOCALE` is an optional single-tag override for reproducible launches; an unsupported
override falls back to English. Empty overrides use the system. Unix locale variables do not
override Windows/macOS UI-language preferences. Tags are read once per process (at most 64,
128 bytes each) and matched against the supported language registry. Changes to the
OS language list itself take effect at the next launch. The web build keeps its existing
English Auto fallback until browser-locale detection is implemented.

API implementation and licensing: [sys-locale](https://github.com/1Password/sys-locale).

Agents use the existing `prefs.set` command through CLI, MCP or the desktop control channel:

```json
{"method":"engine.execute","params":{"command":"prefs.set","params":{"path":"interface.language","value":"ko"}}}
```

## Configuration contract

The embedded bundle lives in `crates/ui-egui/locales/`. `manifest.json` registers languages;
`manifest.schema.json` provides JSON Schema editor completion and field documentation.
`build.rs` validates these files and generates the Rust registry. Adding a bundled language
requires a manifest entry and a TSV file, with no Rust registry edit.

An external pack uses the same manifest format. It can override a bundled language or add a
new one at runtime. This minimal example changes French labels:

```json
{
  "schemaVersion": 1,
  "fallbackLocale": "en",
  "reloadIntervalMs": 1000,
  "languages": [
    {
      "code": "fr",
      "name": "Français",
      "catalog": "fr.tsv",
      "pluralRule": "french",
      "fallback": "en",
      "aliases": [],
      "completeMenus": false
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `schemaVersion` | Required version, currently `1`; unsupported versions fail with a diagnostic. |
| `fallbackLocale` | Default fallback, `en` when omitted. It must exist in the merged registry. |
| `reloadIntervalMs` | Desktop polling interval, 250–60000 ms; default 1000 ms. |
| `code` | Lowercase locale code, at most 16 ASCII bytes; `auto` and `posix` are reserved. |
| `name` | Native language name, at most 128 UTF-8 bytes, without control characters. |
| `catalog` | Flat `.tsv` basename relative to the manifest. A new language requires one. `null` keeps a bundled catalog. |
| `pluralRule` | Required named rule from the table above, using integer counts. |
| `fallback` | Next locale; omission uses the default fallback. English ends the chain. |
| `aliases` | Up to 32 locale aliases, distinct across the registry and not conflicting with another code. |
| `completeMenus` | Require complete source-key coverage in the maintenance check; default `false`. |

An external entry replaces that language's metadata, so include aliases you want to retain.
Other bundled languages remain registered. For each key, lookup tries the external catalog,
then the same locale's embedded catalog, then its configured fallback chain, then the English
source. Each fallback locale applies its own plural rule. Context and command-ID lookups fall
back to plain source-key lookup if there is no specific entry. Unknown fields, duplicate
codes/files/aliases, missing fallback targets and cycles are rejected.

## Desktop file hot reload

Place `manifest.json` and its TSV files in `<PhotoCraft config directory>/Locales`, or set
`PHOTOCRAFT_LOCALES_DIR` to another directory before launching. The config directory follows
`PHOTOCRAFT_CONFIG_DIR` and portable-mode settings; see [configuration paths](control-protocol.md).
The directory does not need to exist for normal startup.

A background worker reads and validates the pack at the configured interval. It compares file
contents, including equal-length edits, and publishes a complete immutable snapshot. The UI
installs snapshots at frame boundaries and wakes only on an update or diagnostic. Filesystem pack loading and parsing run on the worker. Switching snapshots releases old strings after
readers finish; hot reload does not leak strings to manufacture static lifetimes.

- A valid edit updates the selected language without restarting. New languages appear in the
  Preferences dropdown automatically; changing the dropdown follows Apply/Cancel semantics.
- An invalid update or a missing referenced file leaves the last valid snapshot active. The error appears in
  Preferences > Interface and `ui.inspect.localizations.error`; correcting the file clears it.
- **Reload Translations** in Preferences requests an immediate check. Removing `manifest.json`
  deliberately restores the embedded bundle. Unreferenced editor backup files are ignored.
- Save files atomically where possible. When publishing several files, write catalogs first
  and replace the manifest last. Every published snapshot is validated as a whole.

Pack limits: 64 KiB manifest, 64 merged languages, 2 MiB per catalog, 16 MiB total catalog
text, 20000 lines and 10000 entries per catalog. Plain entries preserve placeholders and a
trailing dialog ellipsis; plural messages must contain exactly the configured number of
nonempty forms. The native loader accepts regular files and flat resource names, refusing
symlinks, traversal and oversized inputs. Errors are actionable results, never panic paths.

The filesystem watcher is native-only. Web and agents can install an already supplied pack
through `ui.i18n.load`; the same validator and snapshot mechanism run on WebAssembly.
See [control protocol](control-protocol.md) for command parameters. Programmatically loaded
packs are session-only; a later file-watcher update can replace them.

## Translation file format

Use exactly three tab-separated columns: `context<TAB>source<TAB>translation`.
Blank lines and lines beginning with `#` are ignored. Escape tabs, newlines and backslashes
as `\t`, `\n` and `\\`. Save as UTF-8 (a BOM is accepted).

| Context | Source | Translation |
|---|---|---|
| empty | `Layer` | `Calque` |
| `menu` | `Windows` | context-specific text |
| `@id` | `layer.delete` | command-specific text |
| `@plural` | `{n} item\|{n} items` | `{n} élément\|{n} éléments` |

Keep source keys, contexts, IDs, placeholders and the final `…` intact. Translators may reorder
parameters; inserted values are not recursively interpolated. Keep product names such as
PhotoCraft, technology names and user data unchanged. Translate from meaning using clean-room
work or permissively licensed contributions. Add attribution and adjacent license information
in the same PR; never copy proprietary localisation resources.

## Maintainer workflow

```sh
# Validate resources and require complete coverage for catalogs claiming it.
cargo xtask i18n-coverage
cargo xtask i18n --check
# Review coverage and every missing key as structured JSON.
cargo xtask i18n --report --json
# Create a commented TODO catalog and update the manifest without overwriting existing files.
cargo xtask i18n --init nl --name Nederlands --plural-rule one_other
# Use an external pack directory instead of editing the bundle.
cargo xtask i18n --init nl --name Nederlands --dir /path/to/Locales
cargo xtask i18n --check --dir /path/to/Locales
```

Uncomment TODO rows, leaving the context empty for plain keys, and supply translations. A new
catalog initially uses `completeMenus: false`; missing keys fall back while it is being edited.
Set it to `true` once coverage is complete. CI runs the same maintenance check. Its required
inventory includes menus, UI commands, literal `tl!` expressions (including conditional source
keys), blend modes, brush sections and generated preferences. It is a coverage gate for these
keys, rather than a claim to translate every runtime error or dynamically generated message.

Before submitting, run touched-crate tests, Clippy, layering, WebAssembly and parity checks.
Render representative dialogs with the offscreen snapshot example and inspect the PNGs, including
several language changes in one shell and an edited external translation. See
[development](development.md) and [translation attribution](../ATTRIBUTION.md).

## Fonts and remaining work

Native lazy CJK font loading prioritises the selected Japanese, Korean or Chinese script.
Language changes reset font definitions and the existing loader safely; idle frames retain
caches and newly encountered glyphs load lazily. Fonts are installed on the system or supplied
through optional craft-fonts input, never committed here. See [Fonts](development.md#fonts-craft-fonts).
Web CJK font delivery, browser locale detection, RTL layout and complete engine-message
localisation remain separate work. A translation pack does not provide missing font assets.
