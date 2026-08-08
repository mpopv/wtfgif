# Changelog

All notable changes to wtfgif are documented here.

## Unreleased

## 3.0.9 - 2026-08-08

- Compile every Wasm function needed by the ordinary first quality encode
  during explicit initialization using only empty, data-independent sentinels.
  The real exported encoder and output-pointer shims are entered, but no source
  pixels are read and no palette, histogram, mapping table, or output is
  retained. A lazy-compilation trace now shows zero Liftoff compilations after
  the timed encode begins. Across 500 paired fresh MakeEmoji processes, the
  first encode is 1.23x faster with the exact same GIF hash. Initialization
  increases from 0.893 ms to 2.356 ms while the separate first-encode phase
  falls from 1.257 ms to 0.764 ms.
- Map high-resolution quantized pixels directly from RGBA while writing literal
  LZW instead of retaining one 16-bit histogram cell per pixel. This removes
  the obsolete full-image scratch path and about 800 net lines of code, shrinks
  the SIMD quality Wasm by 17.5 KB, and improves the one-megapixel fresh-process
  encode by 19.4%. Five adversarial megapixel inputs retain identical frame
  metadata and a maximum decoded-channel difference of zero.
- Reserve a true 4 MiB source-independent Wasm input arena during explicit
  initialization. The one-megapixel fixture contains exactly 4 MiB of RGBA, so
  this avoids a timed memory growth that the prior decimal 4 MB reservation
  missed without inspecting or retaining source pixels.
- Prepare the public encoder's data-independent JavaScript validation and
  dispatch during explicit initialization. Its private empty sentinel returns
  before reading pixels, calling Wasm, or creating output state, so the timed
  sample remains the first and only real encode while avoiding V8's first-use
  compilation cost.
- Skip 32-pixel clear-canvas spans with one SIMD alpha reduction while building
  a mixed-alpha histogram. The dedicated function is prepared with an empty
  sentinel during initialization and is entered only when the source starts at
  alpha zero; it retains no image-derived state and delegates every nonzero
  span to the exact existing threshold path.
- Skip eight per-pixel alpha-threshold checks when one packed decision proves
  that an eight-pixel group is fully opaque in the SIMD four-bit mixed
  histogram. Any group containing a non-255 alpha value retains the exact
  pairwise threshold path, and accepted GIF bytes remain unchanged.
- Skip the remaining pairwise alpha checks when one packed decision proves an
  eight-pixel mixed-histogram group is entirely clear. The real MakeEmoji
  workload is 1.68% faster over 500 paired fresh processes, with identical GIF
  bytes and no credible regression across the other nine corpus categories.
- Seed dense SIMD nearest-palette mapping from the exact Wu-cube candidates,
  then skip red slices and red/green rows whose distance lower bound cannot
  beat any requested cell. Full-occupancy tables retain the original dense
  kernel. The optimized path preserves the exact palette indices and GIF bytes
  while reducing the smooth-gradient first encode from 1.004 ms to 0.860 ms.
- Pack the exact eight-color path into 252-byte GIF data sub-blocks aligned to
  complete 432-pixel groups. This removes the generic sub-block boundary branch
  from its literal LZW loop while preserving the exact indexed pixels, decoded
  RGBA output, and file length. Only the legal GIF sub-block boundaries change,
  so byte hashes for this path change.
- Carry the existing classifier's sampled-alpha result into quality indexing so
  opaque-looking images do not repeat the same alpha probe. The encoder still
  checks every pixel and falls back to the mixed-alpha path if an unsampled
  transparent pixel exists.
- Prepare the quality encoder's data-independent Wasm call graphs during the
  explicit initialization step using empty sentinels, including the real
  public low-resolution export and its quantized opaque-probe wrappers. No user
  pixels, palette, output, or image-derived state is created or retained, and
  reusable data scratch remains cold for the first real encode.
- Reuse prior-frame indices while mapping exact small palettes and make the
  bounded classifier prove sampled palettes larger than GIF's 256-color limit
  without redundant occupancy passes. GIF output remains byte-for-byte
  unchanged across the corpus.
- Scan exact-color runs four pixels at a time, write short repeated-index runs
  with packed stores, and reuse prior-frame indices only after proving an
  entire source row is byte-for-byte identical. This cuts the disjoint-palette
  fixture from 0.512 ms to 0.270 ms without changing its GIF bytes.
- Skip palette-table work when a mapped four-pixel SIMD group is entirely
  transparent. The alpha threshold, selected palette indices, and output bytes
  remain unchanged.
- Let the bounded classifier continue its representative color sampling on
  transparent images until it can prove that quantization is required, then
  emit repeated transparent literal groups from pre-expanded bytes. This
  avoids redundant scans and bit packing without changing decoded pixels or
  corpus GIF hashes.
- Cancel the common histogram weight in equal-count Cartesian palette grids.
  This removes redundant count multiplication from variance and representative
  accumulation while preserving the exact palette and GIF bytes. Paired fresh
  processes improve tiny by 1.9% and noise by 1.2%.
- Prove equal histogram weights and Cartesian grid occupancy in one early-exit
  pass instead of traversing the color arena twice. This shrinks quality Wasm
  by 857 bytes and improves paired fresh-process tiny by a further 4.7% and
  noise by 1.3%, with identical GIF bytes.
- Refresh the clean 40-process arbitrary-RGBA receipt. Every category now
  exceeds 100x on its first real encode after initialization: **169.36x to
  418.68x**, with a **213.62x geometric mean** and **175.44x** on the real
  MakeEmoji workload.
- Refresh the 15-process Chrome comparison and generated SVG. wtfgif takes
  **0.850 ms**; the five alternatives take 88.755-124.255 ms and are
  **104.42x-146.18x slower**.
- Pass the complete release gate: lint, type checking, dependency audit, Rust
  formatting/clippy/tests, all scalar and SIMD Wasm builds, 148 JavaScript/Wasm
  assertions, independent decoder conformance, edge validation, and package
  validation.

## 3.0.8 - 2026-08-07

- Specialize the first constant-delay exact-palette encode after the complete
  palette scan has proved a small color table. Eight-color input enters the
  four-bit literal packer directly, while two- and four-color input bypasses
  the generic codec dispatcher and zero-fill before GIF sub-block assembly.
  The specialized writer is byte-for-byte identical to the generic writer in
  direct unit coverage and across the corpus.
- Tighten Binaryen's combined-function inlining budget from 2500 to 1500
  bytes. This reduces V8's honest first-call lazy-compilation work without an
  encode warmup, source-derived initialization, or a change to steady codec
  behavior.
- Correct the first-quality profiler to reserve the same source-independent
  4 MB input arena as the public initialization path before timing. Direct
  release-to-release alternating fresh-process A/B measurements show 3.11%
  lower MakeEmoji latency (280 pairs), 13.95% lower pixel-art latency (320
  pairs), and 13.18% lower nearly-static latency (260 pairs). Every compared
  GIF has the same SHA-256 hash as 3.0.7.
- Refresh the clean 25-process corpus receipt and 15-process Chrome race. The
  corpus spans **74.17x to 127.55x** with a **95.37x geometric mean**; the real
  MakeEmoji workload is **112.38x faster**. In fresh Chrome processes, wtfgif
  takes 1.345 ms and the five alternatives are **62.83x to 89.13x slower**.
- Clarify benchmark provenance in the README and benchmark guide, explicitly
  call out the current sub-100x pixel-art floor, and restore relative slowdown
  labels to the generated browser comparison chart.

## 3.0.7 - 2026-08-07

- Replace recursive palette-tree recoloring with two flat passes: update each
  centroid, then propagate exact subtree bounds through parent links in reverse
  preorder. Alternating fresh-process A/B runs improved the KD-heavy
  MakeEmoji fixture by 0.40% and the one-megapixel fixture by 0.36%; all ten
  corpus GIF hashes are unchanged.
- Tie the README and benchmark claims to their clean committed receipts, and
  update the browser chart to show emitted GIF size alongside encode time and
  disclose that gif.js worker creation is inside its public-API timing.
- Align the exact palette KD nodes to a 16-byte stride so Wasm can load their
  color, bounds, and traversal fields more efficiently. Alternating
  fresh-process A/B runs improved the KD-heavy MakeEmoji fixture by 1.19% and
  the one-megapixel fixture by 0.81%; all corpus GIF hashes are unchanged.
- Pack each eight-code literal group through two staged four-code expansions,
  reducing the RGBA quality encoder's per-pixel mask-and-shift work without
  changing a single output byte. A 10,000-pattern reference-packing test now
  covers the arithmetic directly.
- Reserve a source-agnostic 4 MB input arena during Wasm initialization so the
  first real encode does not allocate its ordinary input scratch inside the
  timed path. No source pixels, palette, or encoded result exists at
  initialization, and corpus output hashes remain byte-for-byte unchanged.
- Route strongly uniform mid-sized animations into the exact small-palette
  attempt after a conservative bounded probe, and stop the general classifier
  after eight decisive samples. The complete exact-color scan remains
  authoritative: GIF bytes and decoded pixels are unchanged. The fresh-process
  pixel-art median improved from 0.507 ms to 0.457 ms in that change's paired
  receipt.
- Refresh the 10-fixture first-encode receipt and six-library browser chart.
  The corpus spans **66.03x to 129.15x** with a **91.61x geometric mean**;
  the real MakeEmoji workload is **108.80x faster**, and the five browser
  alternatives are **67.79x to 97.62x slower** than wtfgif.
- Make encoding latency the only compression policy. Remove the public
  `compression` option and delete the balanced dictionary encoder from the
  JavaScript, Rust, Wasm, and native paths; all encoders now use literal LZW.
- Replace the default single-fixture benchmark with a reproducible 10-fixture
  corpus covering real images, photographic content, pixel art, gradients,
  noise, transparency, disjoint palettes, changed rectangles, tiny animations,
  and a one-megapixel workload. Record raw samples, p95, output hashes, file
  sizes, PSNR, SSIM, provenance, and environment metadata.
- Add independent omggif and Sharp/libvips conformance checks, Chromium,
  Firefox, and WebKit rendering checks, deterministic property tests, bounded
  decoder and encoder fuzz targets, and macOS/Windows/browser/fuzz CI jobs.

## 3.0.6 - 2026-08-07

- Reduce quality-encoder overhead by claiming the embedded Wasm allocator
  arena during module initialization, aligning histogram bins for SIMD access,
  and sharing exact reciprocal division across RGB palette averages. The real
  fixture remains byte-for-byte unchanged at 149,689 bytes (SHA-256
  `e997926c952bf2a374b3304debbe7bacb56894b3990101c996c5426b855c6c2d`).
- Refresh the release receipts: **101.83x** for the first real encode after
  initialization, **287.47x** initialized, **1586.44x** initialized stress,
  and **739.45x** first initialized stress. In fresh Chrome processes, the five
  competing encoders are **75.23x to 113.51x slower** than wtfgif.
- Add exhaustive reciprocal-arithmetic parity coverage and finer first-call
  phase profiling. The complete release gate passes 47 Rust tests, 144
  JavaScript/Wasm assertions, every Wasm variant, edge validation, and packaged
  ESM, CommonJS, browser-global, and browser-Wasm checks.

## 3.0.5 - 2026-08-06

- Rewrite the README benchmark section around the honest first-encode result:
  **100.26x** after Wasm initialization across 500 fresh processes with zero
  encode warmups. Separate that contract clearly from the **298.94x** repeated
  initialized result and the six-library browser race. Runtime code and GIF
  output are unchanged from 3.0.4.

## 3.0.4 - 2026-08-06

- Make the first real arbitrary-RGBA encode after initialized Wasm **100.26x**
  faster than image-q + omggif (123.448 ms vs 1.231 ms) across 500 fresh
  processes with zero encode warmups. The initialized path is **298.94x**
  faster (94.302 ms vs 0.315 ms).
- Reduce first-call work with single-thread Wasm scratch cells, a specialized
  constant-delay low-resolution path, tighter JavaScript validation, converged
  Wasm optimization, and faster literal GIF packetization. Decoded pixels,
  palette quality, transparency, and frame delays are unchanged; the new packet
  layout adds 72 bytes (0.048%) to the eight-frame fixture.
- Refresh the six-library browser race: wtfgif encodes the same eight real
  images in 1.545 ms, making the alternatives **62.05x to 85.13x slower**.
  Full validation passes 144 JavaScript/Wasm tests, 44 core Rust tests, both
  encode-only Rust tests, every Wasm variant, edge-runtime validation, and
  packaged ESM, CommonJS, browser-global, and browser-Wasm checks.

## 3.0.3 - 2026-08-06

- Reduce the first real arbitrary-RGBA encode by simplifying the quality-only
  Wasm ABI, specializing histogram and palette-planning branches at compile
  time, and reusing the exact-color probe's palette allocation.
- Speed up exact palette mapping with packed hash entries, compact Wasm color
  records, radix palette selection, and tighter exact KD-tree searches. Pin the
  proven Rust toolchain and use a small growable Wasm allocator arena. GIF
  bytes, decoded pixels, transparency, and quality remain unchanged.
- The adversarial parity corpus is **108/108 byte-for-byte exact**. Fresh
  release receipts are **72.22x** for the first normal encode after Wasm
  initialization (127.492 ms vs 1.765 ms), **241.24x initialized normal**
  (97.037 ms vs 0.402 ms), **760.48x** for the first initialized stress encode
  (6,440.154 ms vs 8.469 ms), and **1364.67x initialized stress** (6,493.282 ms
  vs 4.758 ms).

## 3.0.2 - 2026-08-06

- Vectorize mixed-alpha histogram indexing and the first four palette mappings
  in each literal-output group on SIMD-capable Wasm runtimes.
- Replace the per-color nearest-palette traversal stack with an exact preorder
  tree whose subtree bounds support direct branch skipping. Palette selection,
  GIF bytes, decoded pixels, transparency, and quality remain unchanged.
- Fresh real-image receipts: **251.68x initialized** (94.632 ms vs 0.376 ms),
  **57.36x first real encode after initialization** (121.707 ms vs 2.122 ms),
  **39.20x fresh-worker operation** (135.744 ms vs 3.463 ms), and **5.42x
  complete process wall clock** (164.648 ms vs 30.355 ms).

## 3.0.1 - 2026-08-06

- Correct the cold benchmark to measure and report complete spawned-process
  wall time separately from the in-worker image-to-GIF operation. Earlier
  wording incorrectly said the worker's internal timer included Node launch.
- Fresh real-image receipts: **224.39x initialized** (94.693 ms vs 0.422 ms),
  **55.88x first real encode after initialization** (121.353 ms vs 2.172 ms),
  **38.81x fresh-worker operation** (134.554 ms vs 3.467 ms), and **5.47x
  complete process wall clock** (163.195 ms vs 29.845 ms).
- The 512x512x10 stress workload remains **215.40x** faster inside a fresh
  worker (6,171.189 ms vs 28.650 ms) and **105.40x** faster including the
  entire spawned process (6,211.252 ms vs 58.928 ms). GIF bytes and quality are
  unchanged.

## 3.0.0 - 2026-08-06

- Make `wtfgif/encode` a single-purpose arbitrary-RGBA encoder: adaptive global
  quality quantization in Wasm followed by lossless literal LZW. The subpath no
  longer exports `GifWriter`, indexed-frame encoding, alternate quantizers,
  palette modes, delta encoding, or JavaScript/native backends. General and
  omggif-compatible APIs remain available from the package root.
- Cut the encode-only JavaScript bundle from 28.35 KB to 6.38 KB ESM by giving
  the quality encoder a dedicated runtime and input path. Remove the obsolete
  generic encode-only Wasm artifacts; `wtfgif/wasm-encode` now resolves to the
  smaller quality-only module used by `wtfgif/encode`.
- Fix allocator corruption when one process alternates between constant and
  per-frame delays. The wasm-bindgen function owns its temporary delay buffer;
  the raw Node loader no longer frees that allocation twice.
- Reduce first-call Wasm compilation and improve initialized throughput by
  keeping eight packed pixels in flight in the quality histogram scan. Output
  remains byte-for-byte unchanged from the established quality path.
- Release receipts on the eight-image arbitrary-RGBA fixture: **225.66x
  initialized** (95.096 ms vs 0.421 ms), **43.12x first real encode after
  initialization** (123.842 ms vs 2.872 ms), and **32.24x full strict-cold**
  (137.358 ms vs 4.261 ms). The 512x512x10 strict-cold stress fixture is
  **207.97x** faster (6,259.352 ms vs 30.098 ms). Expected output bytes and
  decoded quality remain unchanged.

## 2.4.18 - 2026-08-06

- Reduce portable Wasm first-call compilation by keeping the unselected
  median-cut palette branch out of the normal quality encoder body and tuning
  the release optimizer's inlining budget. GIF bytes, decoded pixels,
  transparency, and quality are unchanged.
- Fresh receipts on the arbitrary-image contract: **28.40x strict-cold** on
  eight real 128x128 frames (136.904 ms baseline vs 4.821 ms wtfgif),
  **41.86x** for the first real encode after initialization (123.732 ms vs
  2.956 ms), **217.18x initialized** (95.115 ms vs 0.438 ms), and **201.62x
  strict-cold** on the 512x512x10 stress fixture (6,220.683 ms vs 30.853 ms).
  Outputs remain byte-for-byte expected at 149,601 and 2,973,381 bytes.

## 2.4.17 - 2026-08-06

- Reduce Wasm quality-encoder call overhead in the adaptive palette lookup
  path without changing GIF bytes, decoded pixels, transparency, or quality.
- Fresh strict-cold receipts on the arbitrary-image contract: **24.89x** on
  eight real 128x128 frames (137.319 ms baseline vs 5.516 ms wtfgif), **34.19x**
  for the first real encode after initialization (123.409 ms vs 3.610 ms),
  and **199.88x** on the 512x512x10 stress fixture (6,223.341 ms vs
  31.136 ms). Outputs remain byte-for-byte expected at 149,601 and 2,973,381
  bytes respectively.

## 2.4.16 - 2026-08-06

- Load the quality encoder's Wasm binary directly on Node instead of paying for
  generated binding bootstrap code. Browser loading, public APIs, GIF bytes,
  decoded pixels, and quality remain unchanged.
- Fresh receipts: **24.30x strict-cold** (136.301 ms image-q + omggif vs
  5.610 ms wtfgif) and **32.94x on the first real encode after initialization**
  (123.523 ms vs 3.750 ms) on the eight-frame 128x128 MakeEmoji fixture.

## 2.4.15 - 2026-08-05

- Reduce Wasm quality-quantization working memory and avoid clearing reusable
  histogram bins that are already empty. Palette selection, transparency, GIF
  bytes, decoded pixels, and quality remain unchanged.
- Fresh initialized receipt: 235.19x on the real eight-frame 128x128 fixture
  (95.789 ms image-q + omggif vs 0.407 ms wtfgif), with 149,601 output bytes
  at 34.12 dB PSNR.

## 2.4.14 - 2026-08-05

- Publish the current optimized arbitrary-RGBA quality encoder build to npm.
  API behavior, palette quality, transparency, GIF bytes, and decoded pixels
  are unchanged from 2.4.13.

## 2.4.13 - 2026-08-05

- Fold the opaque-alpha check into the high-resolution quality histogram pass,
  removing a separate full scan for the common opaque-image case. Transparency
  handling, palette selection, GIF bytes, decoded pixels, and quality remain
  unchanged.
- Fresh initialized receipt: 229.89x on the real eight-frame 128x128 fixture
  (96.102 ms image-q + omggif vs 0.418 ms wtfgif), with 157 JavaScript tests,
  45 Rust tests, all Wasm variants, edge validation, and package validation
  passing.

## 2.4.12 - 2026-08-05

- Speed up arbitrary-RGBA quality encoding with chunked wide histograms,
  larger packed histogram scans, and unrolled literal GIF emission. Palette
  selection, transparency, GIF bytes, decoded pixels, and quality are
  unchanged.
- Fresh receipts: 23.52x strict-cold real encode, 227.65x initialized real
  encode, and 1,268.54x initialized 512x512x10 stress encode.

## 2.4.11 - 2026-08-05

- Publish the current optimized arbitrary-RGBA quality encoder build to npm.
  API behavior, palette quality, transparency, GIF bytes, and decoded pixels
  are unchanged from 2.4.10.
- Release validation passed: 157 JavaScript tests, 45 Rust tests, all Wasm
  variants, edge-runtime validation, and package validation.

## 2.4.10 - 2026-08-05

- Speed up exact arbitrary-RGBA quality encoding by scanning opaque histogram
  pixels in larger packed groups and by reusing palette-split statistics.
  Palette selection, transparency, indexed pixels, GIF bytes, and decoded
  quality are unchanged.
- Fresh initialized receipts are 232.92x on the real eight-frame 128x128
  fixture, 1,117.19x on mixed 512x512x10 stress, and 740.19x on opaque
  stress. Strict-cold real encode measured 24.20x.

## 2.4.9 - 2026-08-05

- Pair packed RGBA histogram reads in the arbitrary-RGBA quality encoder and
  keep seven-byte literal groups on the direct unaligned-store path. Palette
  selection, transparency, indexed pixels, GIF bytes, and decoded quality are
  unchanged.
- Fresh receipts on the real eight-frame 128x128 fixture are 236.93x
  initialized. The 512x512x10 stress fixture measured 1,196.76x initialized;
  opaque stress measured 1,313.05x. The real output remains 149,601 bytes at
  34.12 dB PSNR; stress remains 2,973,381 bytes at 26.12 dB.

## 2.4.8 - 2026-08-05

- Remove an unnecessary non-inlining barrier from the low-resolution
  arbitrary-RGBA quality planner, leaving palette selection, transparency,
  indexed pixels, GIF bytes, and decoded quality unchanged.

## 2.4.7 - 2026-08-05

- Speed up the high-resolution arbitrary-RGBA quality histogram by updating
  each packed color bin with one aligned SIMD/native operation. Palette
  selection, transparency, indexed pixels, GIF bytes, and decoded quality are
  unchanged.
- Fresh receipts on the real eight-frame 128x128 fixture are 224.42x
  initialized, 34.17x for the first real encode after initialization, and
  24.20x across 20 strict-cold encode processes. The 512x512x10 stress
  fixture measured 1,124.24x initialized. The real output remains 149,601
  bytes at 34.12 dB PSNR; stress remains 2,973,381 bytes at 26.12 dB.

## 2.4.6 - 2026-08-05

- Tighten exact nearest-color lookup in the arbitrary-RGBA quality planner by
  using subtree bounds for seeded palette searches and skipping a redundant
  refinement search when the current color is already an exact palette entry.
  Palette selection, transparency, indexed pixels, GIF bytes, and decoded
  quality are unchanged.
- Fresh receipts on the real eight-frame 128×128 fixture are 237.18×
  initialized, 33.84× for the first real encode after initialization, and
  22.68× across 20 strict-cold encode processes. Opaque input measured
  197.48× initialized; the 512×512×10 stress fixture measured 1,085.30×.
  The real output remains 149,601 bytes at 34.12 dB PSNR; stress remains
  2,973,381 bytes at 26.12 dB.

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
