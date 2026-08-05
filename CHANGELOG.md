# Changelog

All notable changes to wtfgif are documented here.

## 2.4.5 - 2026-08-05

- Speed up the arbitrary-RGBA quality planner by reusing one exact palette
  lookup, clearing only occupied 4-bit histogram cells, and replacing the
  4-bit histogram comparison sort with an allocation-free linear permutation.
  Palette selection, transparency, indexed pixels, GIF bytes, and quality are
  unchanged.
- Fresh receipts on the real eight-frame 128×128 fixture are 198.18×
  initialized, 33.79× for the first real encode after initialization, and
  24.16× across 20 strict-cold encode processes. Opaque input measured
  191.26× initialized; the 512×512×10 stress fixture measured 1,123.59×.
  The real output remains 149,601 bytes at 34.12 dB PSNR; stress remains
  2,973,381 bytes at 26.12 dB.

## 2.4.4 - 2026-08-05

- Simplify the mixed-alpha histogram dispatch in the arbitrary-RGBA quality
  encoder without changing palette selection, transparency, indexed pixels, or
  GIF bytes.
- Fresh receipts on the real eight-frame 128×128 fixture are 208.80×
  initialized, 32.26× for the first real encode after initialization, and
  23.47× across ten strict-cold encode processes. The 512×512×10 stress
  fixture measured 1,095.80× initialized; output bytes and PSNR remain
  149,601 / 34.12 dB on the real fixture and 2,973,381 / 26.12 dB on stress.

## 2.4.2 - 2026-08-05

- Reuse one prepared Wasm encoder-memory binding across RGBA and indexed encode
  paths, so ordinary calls do not repeatedly rediscover the same scratch
  memory or reset its capacity.
- Dispose one-off composited-frame playback state after copying a frame, while
  keeping the prepared playback API reusable for callers that need it.
- Share Wasm feature/status detection across the full, encode-only, and
  quality-only runtimes. Encoding, decoding, output bytes, and pixel-parity
  contracts are unchanged.

## 2.4.1 - 2026-08-05

- Keep the reusable arbitrary-RGBA input scratch buffer four-byte aligned while
  growing it without zero-initializing bytes that the caller immediately
  overwrites. The encoder output, quality, and pixel-parity contracts are
  unchanged.
- Fresh receipts on the real eight-frame 128×128 fixture are 23.25× strict
  cold, 32.70× for the first encode after initialization, and 196.80× hot;
  output remains 149,601 bytes at 34.12 dB PSNR.

## 2.4.0 - 2026-08-04

- Ship a quality-only Wasm artifact for `wtfgif/encode`, with scalar and SIMD
  Node/browser variants. The encode entry now loads only the ABI needed by the
  arbitrary-RGBA quality path; the full `wtfgif` entry keeps the decoder and
  indexed APIs.
- Remove the synthetic initialization delay and eager ordinary-size scratch
  reservation. Initialization binds Wasm memory only; the first real encode
  reserves exactly the input range it needs, and no source pixels, palette, or
  output are retained.
- Avoid recording a per-pixel 5-bit histogram index when the final palette plan
  cannot consume it. The 4-bit path keeps its direct index stream, and the
  quality/pixel output contract is unchanged.
- Current receipts on the real eight-frame 128×128 RGBA fixture are 190.15×
  initialized, 30.90× for the first real encode after initialization, and
  22.17× for a seven-process strict-cold `wtfgif/encode` run. Opaque input is
  171.02× initialized; the 512×512×10 stress fixture is 1,007.97× initialized.

## 2.3.0 - 2026-08-04

- Keep WebAssembly initialization limited to ordinary scratch allocation; no
  synthetic encode, source pixels, palette, or output is retained.
- Emit the normal quantized 256-color literal stream directly from retained
  histogram cells, and reserve the probe, palette, KD-tree, and color-box
  scratch ranges before the first real encode.
- Current initialized receipts are 206.99× on the real 128×128 RGBA fixture,
  176.20× with opaque input, and 1,014.52× on the 512×512×10 stress fixture.
  The honest initialized-first real receipt is 35.90× because portable Wasm
  still lazily compiles the first encode call.

## 2.2.4 - 2026-08-03

- Add native Node entry points for the quality RGBA, balanced RGBA, and
  balanced indexed encoders, while keeping the portable JavaScript/Wasm path
  as the browser and edge fallback.
- Tighten ordinary GIF parsing and sequential decoding, including reusable
  palette data and literal-stream fast paths. Unusual or malformed GIFs keep
  the compatibility parser and decoder.
- Keep prepared composited RGBA/BGRA frames in Wasm-owned scratch memory when
  the runtime exposes it, avoiding a bindgen result copy without changing the
  exact pixel-parity contract or the fallback API.
- Refresh the initialized arbitrary-RGBA receipt to 217.39× on the real
  MakeEmoji fixture (149,601 bytes / 34.12 dB), 210.52× with opaque input,
  and 1,196.59× on the larger 512×512×10 stress fixture. These are timed only
  after initialization; strict-cold encode-only remains 19.23× because it
  includes process and Wasm startup.
- Refresh the direct indexed receipt to 241.77× (104.11× minimum across the
  2–256-color sweep). Decode remains workload-dependent at 1.08–4.45× in the
  portable drop-in sweep (2.35× geometric mean).

## 2.2.3 - 2026-08-02

- Specialize the ordinary RGBA quality path for opaque input, use compact u32
  histogram/remap accumulators, and avoid redundant color-count and coarse-hint
  work. Palette selection, transparency, nearest-color mapping, and output
  pixels remain exact for the existing contract.
- Improve palette nearest-color lookup with variance-aware KD splits, direct
  subtree bounds, and a guarded literal 9-bit decode path for the common
  min-code-size-8 stream. Keep the portable fallback for every other GIF.
- Keep fast literal LZW as the default indexed writer compression while leaving
  explicit `compression: "balanced"` available for smaller output.
- Fresh initialized receipts are 205.41× on the real-image fixture, 171.20×
  with opaque alpha, and 1,259.25× on the 512×512×10 arbitrary-RGBA stress
  fixture. Output remains 149,601 bytes / 34.12 dB on the real fixture and
  2,973,381 bytes / 26.12 dB on stress.
- The indexed writer is 223.16×. Strict-cold full-entry and encode-only
  receipts are 20.41× and 23.78× because they include process and Wasm
  initialization; decode remains workload-dependent (2.03× on GIGACHAD and
  4.43× on tenor in the targeted receipts).

## 2.2.2 - 2026-08-02

- Keep the reusable byte-addressed encoder input scratch allocation four-byte
  aligned, so packed RGBA loads have a stable base without changing the public
  API or copying fewer source pixels.
- Fresh initialized receipts are 183.98× on the real-image fixture, 163.18×
  with opaque alpha, and 1,316.47× on the 512×512×10 arbitrary-RGBA stress
  fixture. Output bytes and PSNR remain 149,601 / 34.12 dB on the real fixture
  and 2,973,381 / 26.12 dB on stress.
- The indexed writer is 224.64×. Strict-cold encode-only is 23.19× because it
  includes process and Wasm initialization; decode remains workload-dependent
  at 0.86×–3.83× (1.72× geometric mean).

## 2.2.1 - 2026-08-02

- Make exact KD-tree nearest-color searches prune by each subtree's RGB
  bounding box; dense median-cut mapping keeps the cheaper split-plane search.
- Seed the refinement pass from the prior exact assignment and short-circuit
  exact palette-color hits through the existing hash table. These are search
  accelerations only: nearest-color tie-breaking, adaptive palette choices,
  transparency, and output pixels remain unchanged.
- Fresh initialized receipts are 179.74× on the real-image fixture, 161.08×
  with opaque alpha, and 1,265.32× on the 512×512×10 arbitrary-RGBA stress
  fixture. PSNR and output bytes match 2.2.0 (34.12 dB / 149,601 bytes on the
  real fixture; 26.12 dB / 2,973,381 bytes on stress).
- The indexed writer is 223.41×; decode remains workload-dependent at 0.86×–
  3.83× (1.72× geometric mean). Strict-cold encode-only is 23.38× because it
  includes process and Wasm initialization.

## 2.2.0 - 2026-08-01

- Add SIMD Wasm artifacts for Node and browsers with runtime feature detection
  and a scalar fallback, so the fast path is portable rather than native-only.
- Route smooth large-image ramps through the finer 5-bit quality histogram and
  keep the 4-bit/table path for noisy/photo-like inputs; dense 5-bit palettes
  use a compact 4-bit parent-cell map. Add a regression test for the quality
  decision.
- Fuse RGBA palette mapping directly into the literal 9-bit LZW writer for the
  normal 256-color path, removing the intermediate indexed-pixel write/read;
  byte-for-byte parity is covered for transparent and opaque pixels.
- Current initialized receipts are 128.67× on the real-image fixture, 108.69×
  with opaque alpha, and 1,333.26× on the 512×512×10 arbitrary-RGBA stress
  fixture. The latter still has higher measured PSNR and is a valid GIF.
- The current direct indexed writer receipt is 222.07×; decode remains
  workload-dependent at 2.08×–4.27× on the targeted larger fixtures. Strict
  cold startup is 19.04× for the full entry and 22.88× for encode-only because
  both include process and Wasm initialization.

## 2.1.0 - 2026-08-01

- Ship a genuinely encode-only `wtfgif/encode` bundle and Node/browser Wasm
  artifacts. The full `wtfgif` entry keeps its decoder/remux runtime and uses
  the encode module as a lazy fallback, so existing imports keep working.
- Add a fixed quality/global/literal RGBA Wasm entry for the normal arbitrary
  image pipeline. It removes mode dispatch from the hot call graph without
  changing palette quality, transparency handling, or GIF output pixels.
- Keep the quality histogram populated during the exact-color probe, removing
  a redundant prefix scan when arbitrary images exceed the 256-color GIF limit.
- Split the ESM/CJS bundles so importing `wtfgif/encode` no longer evaluates
  the full decoder Wasm loader. The packaged encode entry is now the smallest
  path for image-stitching apps while preserving the existing public options.
- Refresh the receipts: 131.72× initialized real-image RGBA encode, 124.31×
  opaque real-image encode, 633.37× initialized stress encode, 188.01×
  indexed encode, 1.90–4.07× decode, and 21.15× strict-cold real encode from
  the encode-only entry. Strict-cold timing still includes process and Wasm
  startup and is intentionally reported separately from the initialized race.

## 2.0.1 - 2026-08-01

- Document the `wtfgif/encode` entry for encode-only apps; 2.1.0 packages the
  isolated encoder runtime so that entry no longer evaluates decoder Wasm.
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
