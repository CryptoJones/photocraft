# AVIF in PhotoCraft

AVIF still-image import and export are included by default. Open, Open As, drag-and-drop,
Place Embedded, Save As, Export As, CLI conversions/batch, engine Save a Copy, control and
headless MCP use the same codec/I/O implementation. Desktop and browser file filters include
`.avif`. Save As uses codec defaults; Export As exposes the controls below.

| Option | Values | Default |
|---|---|---|
| Colour quality | 1–100 | 90 |
| Speed | 1–10, higher is faster with larger files | 8 |
| Bit depth | Automatic, 8, 10 | Automatic: 8 for U8 documents, 10 otherwise |
| Alpha quality | 1–100, independent of colour quality | 100 |
| Transparency | Keep alpha or flatten over white | Keep |

Exports are **lossy**, including quality 100. There is no lossless export claim or checkbox:
the current Rust encoder does not implement true lossless AV1 encoding
([upstream tracking issue](https://github.com/xiph/rav1e/issues/151)).
The encoder stores full-range identity GBR with no chroma subsampling (4:4:4). U16 and float
inputs are quantized to 10 bits unless 8 bits is explicitly selected. Float values outside
0–1 are clipped, and warnings report this. Alpha is straight and independently encoded;
quality 100 minimizes alpha quantization error but high-depth alpha is still quantized.

RGB ICC profiles are embedded byte-for-byte and restored on import. The I/O layer validates
ICC colour models, converts grayscale and CMYK through the CMS to sRGB, and uses the existing
colour-managed compositor for Lab documents. Disabling ICC embedding in the codec export
options converts document colours to sRGB before omitting the profile. AVIF without ICC uses
CICP metadata: sRGB, linear sRGB, Display P3 with sRGB transfer, and Rec.2020 with BT.709-family
transfer are tagged using CMS profiles. Unspecified primaries/transfer use the documented
sRGB fallback. PQ/HLG and other unsupported colour encodings require a supported ICC profile
or produce an actionable error; they are never silently displayed as sRGB.

The standalone raw codec requires an RGB ICC profile when encoding non-sRGB CICP
pixels; it rejects an unprofiled direct re-encode instead of relabelling the values.
Document I/O supplies the appropriate profile, so normal app and agent saves preserve
their colour meaning.

Import supports 8/10/12-bit AV1 still pictures, including general AV1 sequence headers
without the optional `still_picture` flag ([AVIF 1.2 §2.1](https://aomediacodec.github.io/av1-avif/v1.2.0.html#av1-image-item)),
alpha, monochrome (expanded to RGB),
full/limited range, and common BT.601/BT.709/BT.2020 nonconstant-luminance matrices. High-depth
samples are scaled to full-range U16 storage. Subsampled chroma currently uses nearest-neighbour
upsampling; import quality differs from viewers with filtered chroma reconstruction.
Sequences, grids/derived images, essential unknown properties, rotation/mirror/clean-aperture
transforms and unsupported matrices are rejected. EXIF, XMP, DPI and text are not preserved by
this initial AVIF path; exports warn about losses. Gain maps and non-alpha auxiliary items are
not imported. No HDR gain-map reconstruction is claimed.

Dimensions and allocation budgets are checked before pixel decode. The decoder reserves a
conservative 64 bytes per declared pixel for working memory; a tight allocation budget can
therefore reject an image whose final RGB buffer alone would fit. Export is capped at 120 MP
and 65536 pixels per side. Third-party codec calls are guarded against escaped Rust panics on
native builds. Browser wasm has the existing panic-abort limitation; structural checks and
codec limits are still applied there.

## Agent and CLI options

```text
photocraft-cli convert input.png output.avif --quality 90 --avif-speed 8 --avif-depth 10 --avif-alpha-quality 100
```

`run` and `batch` accept the same CLI flags. Engine `file.saveACopy` and file/batch exports use
`avifQuality`, `avifSpeed`, `avifDepth` (0, 8, 10), `avifAlphaQuality`; JPEG's legacy `quality`
0–12 scale remains separate. Headless RPC `doc.save` and MCP `doc_save` accept those four
parameters; their generic `quality` is 1–100 and also sets AVIF colour quality. Live bridge
MCP saves use app defaults and reject headless-only option requests explicitly. For custom
live settings, use the Export As dialog and `ui.dialog.set`.

## Dependencies and verification

Reviewed on 2026-10-09 against upstream documentation and crate sources:

* [`ravif` 0.13.0](https://docs.rs/ravif/0.13.0/ravif/struct.Encoder.html), BSD-3-Clause,
  MSRV 1.85, backed by Rust `rav1e`. Assembly and threading features are disabled.
* [`sqzer-rav1d` 0.1.0](https://crates.io/crates/sqzer-rav1d/0.1.0), BSD-2-Clause,
  MSRV 1.79, fork of upstream main d3d1cd6 carrying the upstream safe Rust API and wasm
  libc shim. No local unsafe code; dependency internals contain unsafe Rust. Assembly is disabled.
* [`avif-parse` 2.1.0](https://github.com/kornelski/avif-parse), MPL-2.0, MSRV 1.90,
  fallible container/AV1 header parsing. No source modifications are vendored.
* `zenavif` / `rav1d-safe` were considered and rejected because their current published
  licensing is AGPL/commercial. `avif-decode` 3.0 requires Rust 1.98 and enables x86 assembly;
  it does not fit PhotoCraft's Rust 1.95 baseline.

No system codec libraries, NASM installation, external image command, or JS UI are required.
The scalar Rust path is architecture-portable; Windows x64 and wasm compilation are verified
locally. macOS, Linux, ARM and Windows x86 execution need their CI/platform runs before being
claimed verified. Wasm compilation alone does not establish browser runtime behaviour or
download-size budget compliance.

Synthetic tests cover quality/depth/profile/alpha, independent 12-bit libaom output and
libdav1d decode pixels, sniffing, truncation, hostile dimensions and validation. Fixture
provenance and reproducible generation live in
[`crates/codecs/tests/fixtures/avif/README.md`](../crates/codecs/tests/fixtures/avif/README.md).
The test's maximum source round-trip channel error at quality 100 is bounded by 0.012.

Local validation on Windows x64 (2026-10-09): eight AVIF correctness/oracle tests,
27 codec fidelity tests, 32 I/O flat tests and five export-dialog tests pass. The
export drawing test verifies destination selection, decodable output and the
displayed saved path. A user-supplied 920 × 1280 still with a general AV1 header
also decoded successfully; that private image is not a committed fixture.
Engine adversarial `panic_hunt`, dependency layers, all workspace wasm checks,
the actual web app check, disabled-feature check and strict clippy for the
codec/I/O/UI/automation/CLI/desktop targets pass. The AVIF Export As dialog was
rendered offscreen and visually inspected.

The full pinned codec/I/O release corpus suite passes, including PSD oracle and
round-trip floors, smart/text/TIFF corpora and adversarial mutations. The generic
mutation seed generator uses unprofiled grayscale for AVIF: raw profiled-gray
conversion is deliberately rejected, while the RGB seeds still exercise ICC boxes.

The final full UI unit run passes 917 tests (three ignored), with only an unchanged
eyedropper cursor assertion failure excluded after reproducing it alone. Strict
clippy including engine unit tests hits two existing
`manual_range_contains` warnings under Rust 1.97; those engine files are unchanged.

A 24 MP synthetic RGB8 gradient at quality 90/speed 8 in the release scalar,
single-thread path took 18445 ms to encode and 1244 ms to decode (15486 bytes).
This run overlapped compiler activity and establishes a diagnostic timing, not
an enforced budget or representative photographic compression ratio. There is
no before measurement because this build adds AVIF decoding. Reproduce with:

```text
cargo test --release -p photocraft-codecs --features corpus,heif --test avif avif_24mp_release_timing -- --ignored --nocapture
```
