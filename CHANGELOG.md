# Changelog

All notable changes to wtfgif are documented here.

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
