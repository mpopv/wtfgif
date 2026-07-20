# Changelog

All notable changes to wtfgif are documented here.

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
