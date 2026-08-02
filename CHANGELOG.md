# Changelog

All notable changes to wtfgif are documented here.

## 2.0.1 - 2026-08-01

- Add the `wtfgif/encode` entry for encode-only apps; it avoids parsing the
  decoder and structural-remux exports during startup.
- Speed up adaptive RGBA quantization with packed histogram indexing,
  unrolled pixel mapping, selection-based palette trees, reusable quantized
  color storage, and one-pass exact-palette indexing. The quality contract is
  unchanged: the real-image fixture remains 34.12 dB PSNR.
- Replace dense median-cut comparison selection with byte-axis weighted
  partitioning and one-pass child statistics; packed color records keep the
  large-image arena compact without changing the adaptive palette contract.
- Add an opaque-input fast scan for `alphaThreshold: 0`, removing per-pixel
  alpha checks while preserving the same adaptive palette and literal output.
- Feed two packed bytes at a time to the Wasm LZW decoder on 32-bit targets,
  reducing bit-buffer refill overhead while preserving exact decoded pixels.
- Select only the dominant histogram colors before sorting the palette tail;
  fixture outputs remain byte-for-byte identical.
- Reuse the histogram-to-palette table without clearing bins that cannot be
  referenced, seed refinement lookups from their first-pass palette index, and
  remove overflow checks from the bounded 4-bit histogram averages.
- Defer histogram allocation for exact-palette RGBA inputs and recycle the
  single full-size indexed scratch buffer through quantization, preserving
  output bytes while removing avoidable scans, growth, and allocation.
- Choose each KD split axis from the largest current RGB range and seed exact
  nearest-color searches from each median-cut representative; the nearest
  result remains exact.
- Unroll eight packed pixels in the quality histogram scans, preserving the
  same adaptive palette and output pixels.
- Refresh the benchmark receipts: 122.57× initialized real-image encode,
  120.23× opaque real-image encode, 641.93× initialized stress encode,
  180.48× already-indexed encode, 1.90–4.46× decode, and 20.46×/21.55×
  strict-cold real encode (164.36× on strict-cold stress). Strict-cold timing
  remains a separate startup-inclusive contract.

## 2.0.0 - 2026-08-01

### Breaking changes

- Remove the old `WasmWebModule` warmup-primer hooks
  (`prepare_reencode_hot_path`, `reencode_hot_path_primer`, and
  `remux_hot_path_primer`). WebAssembly initialization now only loads the
  module; callers do not need to run synthetic encode, decode, or remux work.
  `initializeWasmGlobally()` and `initializeWasmModule()` remain the complete
  startup API.

### Performance and documentation

- Keep arbitrary RGBA encoding on one quality-first adaptive-palette pipeline
  with literal LZW, while reusing input, output, delay, histogram, and palette
  scratch storage.
- Stop exact-color bookkeeping as soon as a source exceeds the GIF palette
  limit, use branch-free histogram accumulation for the remainder, and use
  representative-seeded KD-tree searches for exact palette assignment.
- Lower the Wasm decode handoff threshold for medium partial frames after
  parity-checked measurements showed the Rust decoder wins there too.
- Add `npm run bench:rgba:cold` and document initialized, strict-cold, indexed,
  and decode measurements separately so a remux shortcut cannot be mistaken
  for arbitrary-image encoding.
- Refresh the release receipts on the real-image fixture: 147.49× initialized
  RGBA encode, 18.96× strict-cold RGBA encode, 180.28× indexed encode, 1.86×
  GIGACHAD decode, and 4.31× tenor decode, with exact decoded-pixel checks.

## 1.4.0 - 2026-07-20

- Decouple RGBA quantization from LZW compression with explicit `exact`,
  `fast`, and `quality` modes.
- Add fused arbitrary-RGBA Wasm encoding with fixed RGB332 or adaptive
  median-cut palette generation.
- Add global and per-frame local palette modes. Local exact palettes preserve
  independently representable frames even when their combined colors exceed
  256.
- Make quality quantization with literal LZW the arbitrary-RGBA default. The
  indexed GIF pixels remain lossless after quantization, while the compact
  `compression: "balanced"` encoder stays available as an explicit opt-in;
  legacy `compression: "fast"` remains supported for compatibility.
- Remove the browser racer's lower-quality profile: it now measures one
  adaptive quality-palette pipeline, with literal LZW emitted directly into
  the final GIF buffer.
- Remove initialization-time warmup primers; Wasm initialization now only loads
  the module and does no unrelated encode, decode, or remux work.
- Reuse Wasm input/output and quantization scratch buffers, and fuse exact-color
  detection with the adaptive histogram so repeated arbitrary-RGBA encodes do
  not allocate or rescan more than necessary.
- Carry median-cut box statistics through each split and emit local/global
  literal payloads directly, removing avoidable quantizer scans and compressed
  buffer copies without changing decoded pixels.
- Replace repeated median-cut full sorts with weighted selection and keep exact
  nearest-color assignments, preserving quality while reducing palette cost on
  large arbitrary-RGBA inputs.
- Keep median-cut colors in one arena and cache final box representatives,
  removing per-split allocations and repeated weighted sums without changing
  palette membership.
- Stop exact-color detection as soon as an input exceeds the GIF palette limit,
  then finish the same histogram in a branch-free accumulation pass.
- Seed exact palette KD-tree searches from each median-cut representative and
  prune far branches before pushing them, reducing nearest-color work without
  changing assignments.
- Lower the Wasm decode handoff cutoff for medium partial frames after parity-
  checked measurements showed it beats the JavaScript decoder there too.
- Validate arbitrary-RGBA encoding in JavaScript, WebAssembly, Cloudflare
  Workers, and Vercel Edge, with end-to-end RGBA benchmarks and quality
  measurements.
- Make the default benchmark and live browser racer measure real static images
  through palette generation, pixel mapping, and GIF compression. The racer
  also accepts multiple user images and reports speed, bytes, and quality.
- Restore the canvas between independent transparent RGBA frames so global-
  palette output remains compositionally correct across animation playback.

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
