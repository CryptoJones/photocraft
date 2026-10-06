# Native release size

Native releases ship separate Apple silicon and Intel Mac builds, Windows x64/x86/ARM64
builds, and Linux x86_64/aarch64 builds. GUI packages and CLI archives are separate. Windows
x86 means 32-bit; Linux aarch64 and Mac Apple silicon mean ARM64.

## Implementation and validation contract

Packaging work was split among Mac, Windows and Linux agents with exclusive ownership of
their respective `packaging/<platform>` files. They returned source changes without builds.
The coordinator owns profile settings, workflows, documentation, integration, builds and PR.

No image formats, fonts or glyph coverage, accessibility, GPU fallbacks, automation commands,
or native panic recovery may be removed to meet a size target. Preserve licenses, signing,
notarization, minimum OS versions and architecture checks. Diagnostic symbols stay outside
release downloads. Keep regular `release` unchanged as the same-source performance oracle.

Validation batches:

1. Packaging fixture tests, shell/XML/workflow/profile checks and normal release baseline.
2. Build `native-release`; compare stripped sizes and representative image-processing timings
   against the same-source baseline, not an older published release.
3. Package/verify each Mac architecture centrally; PR jobs verify Mac packages, Windows
   MSI/portable/CLI payloads and both Linux architecture archives. Windows ARM64 has its
   existing install/run CI lane. Release CI still creates every Linux installer and Flatpak.

`python3 scripts/test_release_packaging.py` exercises package boundaries, architecture mapping,
licenses, executable mode, diagnostics separation, failed stripping and invalid Mac arguments
without Rust builds. `.github/workflows/native-packaging.yml` checks real compiled artifacts.

## Choices

`native-release` uses fat LTO and `opt-level="s"`, with speed optimization retained for pixel,
codec, vector and text crates. It keeps `panic="unwind"` and line-table diagnostics. Packaging
extracts dSYMs/ELF debug files before stripping; Windows retains matching PDBs. CI diagnostics
are separate 30-day Actions artifacts, not GitHub Release assets; preserve them longer for
long-term crash support. They are not secret storage.

ZIP compression is Optimal, gzip tarballs use level 9 and DMGs retain zlib level 9. Installer
compression/runtime requirements remain compatible. Native bundled font bytes already share
one static source between UI and text; verify their copy count when measuring new binaries.
Default fallback fonts remain: blindly removing them loses symbols/emoji/script coverage.
GPU, SVG, plugin and codec dependencies provide supported functionality and are retained.
`opt-level="z"`, native panic abort and executable packing are not used.

## Baseline evidence

The SHA256-verified published v0.2.0 GUI executable was 78.10 MiB Mac universal, comprising
37.23 MiB Apple silicon and 40.85 MiB Intel; Windows x64 was 44.14 MiB and x86 36.95 MiB;
Linux x86_64 was 42.50 MiB and aarch64 36.98 MiB. These are historic shipped measurements,
not a same-source compiler comparison. Windows/Linux GUI downloads included the CLI then.

Stripping local symbols from a copy of the universal Mac executable saved 8.27 MiB installed,
but only 1.28 MiB in a gzip proxy because symbol names compress well. Every shipped GUI
architecture contained two copies of the four bundled fonts (1.45 MiB redundant payload per
architecture); the existing source sharing fix predates this packaging change. The Windows
x64 ZIP's CLI entry alone contributed 10.33 MiB compressed. Installed size and download size
must be reported separately; app bundle resources/signatures are additional to executable size.
