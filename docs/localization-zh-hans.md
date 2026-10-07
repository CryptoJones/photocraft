# Simplified Chinese catalog

The `zh-hans` catalog began with the English source keys in
[PhotoCraft PR #169](https://github.com/storytold/photocraft/pull/169), commit
`c1fb8909c4b3b176eead12d66b04338a103cf63d`. The Chinese wording is original
and uses ordinary image-editing terminology; no proprietary translation resources
were extracted or copied. Contributions use the repository's MIT OR Apache-2.0 license.

## Current status

The Simplified Chinese catalog is registered as `zh-hans` and can be selected in
Preferences. Its wording targets Mainland China usage. Both Chinese catalogs
now cover every English base Fluent message ID and all four command-specific
IDs (2,434 messages per Chinese catalog). The wording follows the table below; no proprietary
catalog was used. The Traditional Chinese catalog targets Taiwan usage and was
proofread separately; script conversion alone is insufficient.

## Files and integration

- `crates/ui-egui/src/i18n/locales/zh-CN/messages.ftl` is the editable Fluent catalog and the
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

## Regional terminology

| English | Mainland Simplified | Taiwan Traditional |
| --- | --- | --- |
| Layer / Layer Comp | 图层 / 图层复合 | 圖層 / 圖層構圖 |
| Mask / Clipping Mask | 蒙版 / 剪贴蒙版 | 遮色片 / 剪裁遮色片 |
| Selection / Feather | 选区 / 羽化 | 選取範圍 / 羽化 |
| Blend Mode / Opacity | 混合模式 / 不透明度 | 混合模式 / 不透明度 |
| Adjustment Layer | 调整图层 | 調整圖層 |
| Smart Object / Smart Filter | 智能对象 / 智能滤镜 | 智慧型物件 / 智慧型濾鏡 |
| Canvas / Artboard | 画布 / 画板 | 畫布 / 工作區域 |
| Export / Preferences | 导出 / 首选项 | 匯出 / 偏好設定 |

Terminology was cross-checked against public [Mainland selection guidance](https://helpx.adobe.com/cn/photoshop/desktop/make-selections/refine-modify-selections/refine-and-soften-selection-edges.html)
and [Taiwan layer-mask guidance](https://helpx.adobe.com/tw/photoshop/using/editing-layer-masks.html).
The PhotoCraft messages themselves are original translations.

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
requests today. For translator-friendly Fluent plural editing, evaluate Mozilla
Pontoon first: it presents target-locale plural categories and example numbers.
Weblate offers a GitHub pull request workflow and Fluent syntax checks, but its
Fluent editor does not expose plural variants as separate fields. Keep
`cargo xtask i18n check` and the UI build as merge gates with either service.
No translation service is connected yet.
The [Pontoon setup guide](pontoon.md) records the project configuration, and
the [menu gallery](images/i18n/README.md) contains ten menus in each Chinese locale.

References: [Pontoon's Fluent editor](https://blog.mozilla.org/l10n/2019/04/11/implementing-fluent-in-a-localization-tool/),
[Weblate's Fluent format support](https://docs.weblate.org/en/latest/formats/fluent.html).
