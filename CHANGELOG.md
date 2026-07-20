# Changelog

All notable changes to wtfgif are documented here.

## 1.4.0 - 2026-07-20

- Decouple RGBA quantization from LZW compression with explicit `exact`,
  `fast`, and `quality` modes.
- Add fused arbitrary-RGBA Wasm encoding with fixed RGB332 or adaptive
  median-cut palette generation.
- Add global and per-frame local palette modes. Local exact palettes preserve
  independently representable frames even when their combined colors exceed
  256.
- Preserve the existing fast-mode default: callers must explicitly opt into
  lossy quantization rather than silently losing colors.
- Validate arbitrary-RGBA encoding in JavaScript, WebAssembly, Cloudflare
  Workers, and Vercel Edge, with end-to-end RGBA benchmarks and quality
  measurements.

## 1.3.0 - 2026-07-19

- Add separately optimized, pixel-perfect `GifWriter` encode and `GifReader`
  decode paths for normal typed-buffer usage.
- Add opt-in `compression: "fast"` encoding. The included 256-color,
  12-frame benchmark measures 102.18x faster than omggif with exact RGBA
  parity; its output is 1.37x larger.
- Route substantial ordinary `decodeAndBlitFrameRGBA()` and
  `decodeAndBlitFrameBGRA()` calls through reusable Rust/WebAssembly scratch
  memory while preserving omggif's caller-buffer and transparency behavior.
- Measure encode and decode independently with public drop-in APIs and
  mandatory pixel-parity checks.
- Simplify the README and benchmark documentation around initialization,
  encoding, decoding, measured speedups, and the fast encoder's size tradeoff.

## 1.2.0 - 2026-07-19

- Add `compileGif()`, an immutable compiled-GIF representation that retains
  original palettes, frame rectangles, extensions, and compressed LZW data.
- Add `CompiledGif.withDelays()` and `retimeGifPixelPerfect()` for timing edits
  that patch metadata without decoding, quantizing, or recompressing pixels.
- Add `CompiledGif.withLoop()` with Netscape and ANIMEXTS loop support.
- Add strict `reverseFrames()` and MakeEmoji-compatible `boomerangFrames()`
  linker paths for structurally independent full-canvas animations.
- Preserve exact pixels and compressed frame payloads across the real fixture
  corpus, including local palettes, interlacing, transparency, disposal,
  comments, and application extensions.
- Add a MakeEmoji-shaped timing export benchmark. The page-load-compiled path
  measured at least 951x faster on every fixture.
- Benchmark proven-safe reverse at least 5,329x faster and boomerang at least
  6,866x faster while checking exact mapped pixels and delays.

## 1.1.0 - 2026-07-19

- Run the pixel-perfect one-shot Rust reencoder and all-frame RGBA decoder
  through WebAssembly in browsers and edge runtimes.
- Add static Wasm initialization for Cloudflare Workers and Vercel Edge.
- Add `remuxGifPixelPerfect()`, a portable lossless Rust/Wasm path that
  preserves original compressed frame payloads and exceeds 100x on every
  page-load-prepared benchmark fixture.
- Prepare generic Wasm and public API hot paths during explicit page-load
  initialization.
- Add explicit `rust` and `wasm` encoder backend choices plus fast-backend
  status reporting.
- Validate real reencoding and decoded-pixel parity in the packaged browser
  build, Cloudflare workerd, Vercel Edge VM, and the full GIF corpus.
- Add an interactive GitHub Pages race across all 12 benchmark fixtures with
  live browser timing and post-timer full RGBA parity verification.

## 1.0.0 - 2026-07-19

First public release.

### Highlights

- Drop-in `GifReader` and `GifWriter` APIs for core omggif usage, including
  plain arrays, typed arrays, Node buffers, CommonJS, ESM, and TypeScript.
- Portable JavaScript fallback plus packaged WebAssembly builds for Node and
  browsers.
- Higher-level helpers for indexed and RGBA frame encoding, composited frame
  preparation, delta frames, and configurable compression.
- Pixel-parity tests against omggif and installed-tarball validation across
  CommonJS, ESM, browser exports, and WebAssembly loading.
- Experimental injectable Node addon for one-off decode and pixel-perfect
  reencoding research. Prebuilt native binaries are not part of 1.0.0.
