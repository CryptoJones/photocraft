# Simplified Chinese catalog

The `zh-hans` catalog is registered as **简体中文** in the shared i18n foundation
introduced by [PhotoCraft PR #169](https://github.com/storytold/photocraft/pull/169).
Simplified Chinese support landed in
[PR #282](https://github.com/storytold/photocraft/pull/282), after the v0.2.0 release.
That release does not contain the language selector or this catalog; use a build
that includes the localisation changes.

The Chinese wording is original and uses ordinary image-editing terminology;
no proprietary translation resources were extracted or copied. Contributions
use the repository's MIT OR Apache-2.0 license.

## Selecting Simplified Chinese

Select **Preferences > Interface > Language > 简体中文**, or set
`interface.language` to `zh-hans` through the existing `prefs.set` command.
The native build's **Auto** setting reads `LANG`/`LC_*`, the macOS preferred
languages and the Windows user locale. `zh`, `zh-CN`, `zh-SG` and `zh-Hans`
variants resolve to this catalog. Traditional Chinese locales (`zh-TW`, `zh-HK`,
`zh-MO`, `zh-Hant`) resolve to the separate `zh-hant` catalog.

The web build does not detect the browser language automatically; choose the
language manually there.

## Files and integration

- `crates/ui-egui/src/i18n/zh-hans.tsv` contains the translations, independent of
  the lookup implementation. Its UTF-8 columns are `context<TAB>source<TAB>translation`.
- `crates/ui-egui/src/i18n/mod.rs` registers `zh-hans` with one plural form and
  `complete_menus: true`. Shared tests enforce coverage of every menu string,
  `tl!` literal, blend mode, brush section and generated preference label.
- Shortcut templates retain `{key}` so the shell supplies the correct key for
  the current platform. Layer-count messages use one Chinese plural form.

Keep message IDs, command IDs, variables, and context distinctions intact.
Retain the trailing `…` on commands that open a dialog. Missing translations use
the framework's English fallback. User-supplied names and document data are not
translated. Product names and technology names such as PhotoCraft, ArtCraft,
OpenType, RGB, CMYK and Lab retain their spelling. Status messages, errors and
automation output remain English as described in [UI design](ui-design.md#localisation);
complete catalog coverage does not claim those strings are translated.

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
duplicate keys, placeholders, ellipses, menu coverage, `tl!` literals, brush
sections and blend modes. Chinese-specific tests cover locale selection, fallback,
plural messages, formatted labels and the painted brush section rows and heading. Render and inspect the menus, Preferences and representative
dialogs with the existing offscreen `snapshot` example.

Font delivery is separate from the translation data. The catalog adds no font
assets; the shell's CJK fallback loads fonts separately. Verify missing glyphs,
label layout and platform-specific shortcuts visually on each target platform,
and verify web glyph coverage separately.

When English labels change, update their source keys and translations together,
preserving contexts and placeholders. The complete-catalog tests catch missing
keys, and the Simplified Chinese regression test checks dynamic shortcut labels,
layer counts and English fallback. Review changed source meanings against the
current English UI.
