# Simplified Chinese catalog

The `zh-hans` catalog began with the English source keys in
[PhotoCraft PR #169](https://github.com/storytold/photocraft/pull/169), commit
`c1fb8909c4b3b176eead12d66b04338a103cf63d`. The Chinese wording is original
and uses ordinary image-editing terminology; no proprietary translation resources
were extracted or copied. Contributions use the repository's MIT OR Apache-2.0 license.

## Current status

The Simplified Chinese catalog is registered as `zh-hans` and can be selected in
Preferences. The current catalog covers every source, context, and command key in
the Japanese, Traditional Chinese, Spanish, and Czech catalogs. The 133 entries
added during the Fluent migration use original wording from PhotoCraft's English
UI. Their terminology follows the table below; no proprietary catalog was used.

## Files and integration

- `crates/ui-egui/src/i18n/zh-hans.ftl` is the editable Fluent catalog and the
  translation source of truth. Message IDs correspond to entries in the English
  catalog and are audited by `cargo xtask i18n`.
- Select **Preferences > Interface > Language > 简体中文**, or set
  `interface.language` to `zh-hans` through the existing `prefs.set` command.

Keep message IDs, command IDs, variables, and context distinctions intact.
Retain the trailing `…` on commands that open a dialog. Missing translations use
the framework's English fallback. User-supplied names and document data are not
translated. Product names and technology names such as PhotoCraft, ArtCraft,
OpenType, RGB, CMYK and Lab retain their spelling.

The locale resolver recognizes `zh`, `zh-CN`, `zh-SG` and `zh-Hans` variants.
Traditional Chinese locales (`zh-TW`, `zh-HK`, `zh-MO`, `zh-Hant`) select the
Traditional Chinese catalog. Native automatic detection uses platform locale
settings; the web build currently needs manual selection.

## Terminology

| English | Simplified Chinese |
| --- | --- |
| Layer / Layer Comp | 图层 / 图层复合 |
| Mask / Clipping Mask | 蒙版 / 剪贴蒙版 |
| Selection / Feather | 选区 / 羽化 |
| Blend Mode / Opacity | 混合模式 / 不透明度 |
| Adjustment Layer | 调整图层 |
| Smart Object / Smart Filter | 智能对象 / 智能滤镜 |
| Canvas / Artboard | 画布 / 画板 |
| Brush / Stroke | 画笔 / 描边 |
| Fill / Gradient | 填充 / 渐变 |
| Path / Rasterize | 路径 / 栅格化 |
| Preset / Swatch | 预设 / 色板 |
| Export / Preferences | 导出 / 首选项 |

## Validation and maintenance

Run `cargo test -p photocraft-ui-egui`, the touched-crate all-target clippy check,
`cargo xtask layers` and `cargo xtask wasm`. The shared catalog tests validate
duplicate keys, placeholders, ellipses, menu coverage, `tl!` literals and blend
modes. Chinese-specific tests cover locale selection, fallback, plural messages
and formatted labels. Render and inspect the menus, Preferences and representative
dialogs with the existing offscreen `snapshot` example.

Font delivery is separate from the translation data. This contribution adds no
font assets. The native UI uses locale-aware system CJK fallback fonts; verify
web glyph coverage separately.

Current offscreen captures at 1440 × 900 show the [New Document dialog](images/zh-hans-new-document.png),
[Web presets](images/zh-hans-web-presets.png), and [Interface Preferences](images/zh-hans-preferences.png).
They contain no document artwork. The captured labels fit within their controls;
device names and video standards retain their source spelling. The default
document name is translated for display while the engine value remains stable.

The Fluent migration converted the earlier TSV catalog mechanically, keeping
source meaning, command IDs, placeholder names, and context distinctions. Review
new UI strings against the English Fluent catalog and use the catalog analyzer to
check coverage.

## Proofreading workflow

The catalogs are plain Fluent files and can be reviewed through GitHub pull
requests today. Weblate is the recommended next step for community proofreading:
configure its Fluent component with `en.ftl` as the source catalog and each
language's `.ftl` as a translation, then have it submit translation pull requests.
Keep `cargo xtask i18n check` and the UI build as merge gates. Weblate's Fluent
editor validates syntax, but plural variants remain part of the Fluent message
syntax rather than separate plural fields. No Weblate project is connected yet.
