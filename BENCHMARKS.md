# Benchmarks

Performance claims are only meaningful when the backend, cache state, fixtures,
and correctness checks are explicit.

## Experimental cold pixel-perfect reencoding

The current research checkpoint starts a fresh process for each sample and
reencodes every GIF image descriptor with the experimental native addon. The
timer excludes the decoded-pixel verification that follows each sample. Every
output and source GIF is then decoded and compared; any pixel mismatch aborts
the run.

Command:

```bash
BENCH_ITERATIONS=31 BENCH_REENCODE_ONLY=1 npm run bench
```

31 fresh-process samples per implementation and fixture:

| Fixture | wtfgif faster than omggif |
| --- | ---: |
| 18d | 142.65x |
| Clap | 100.97x |
| Homer | 191.85x |
| Chip | 200.35x |
| GIG | 180.09x |
| NOD | 111.37x |
| Proud | 229.79x |
| cat | 180.53x |
| excuse | 117.74x |
| blob | 126.32x |
| parrot | 149.29x |
| tenor | 214.14x |
| Geometric mean | 156.69x |

These are experimental-addon results on the development machine, not a promise
for the portable npm package or other hardware. The 1.0.0 npm artifact ships
JavaScript and WebAssembly; it does not ship a prebuilt native addon.

## Page-load-prepared portable pixel-perfect remux

This is the web product benchmark. Wasm initialization and generic hot-path
preparation happen before the clock, as they would during page load. Each timed
sample is still the first real GIF processed in a fresh process. Preparation
uses unrelated generated GIFs and cannot cache a fixture's input or output.

wtfgif validates and rewrites the GIF container while preserving original LZW
frame payloads. omggif decodes and freshly recompresses the frames. The
externally tested contract is the same—an owned GIF with identical decoded
pixels—but the algorithms intentionally differ.

```bash
BENCH_ITERATIONS=31 BENCH_REENCODE_ONLY=1 \
WTFGIF_BENCH_BACKEND=wasm \
WTFGIF_PREPARE_WASM_AT_PAGE_LOAD=1 \
WTFGIF_REENCODE_MODE=remux npm run bench
```

| Fixture | wtfgif faster than omggif |
| --- | ---: |
| 18d | 738.64x |
| Clap | 309.84x |
| Homer | 983.53x |
| Chip | 768.77x |
| GIG | 607.23x |
| NOD | 512.23x |
| Proud | 681.13x |
| cat | 519.37x |
| excuse | 480.06x |
| blob | 955.88x |
| parrot | 244.09x |
| tenor | 1,238.09x |
| Geometric mean | 609.73x |

Every fixture clears 100x. The slowest is partyparrot at 244.09x.

The excluded initialization plus generic preparation cost was measured
separately across 31 fresh processes on the same machine: 8.64 ms median,
9.86 ms p95, and 9.91 ms maximum.

## Cold portable Rust/WebAssembly reencoding

The same pixel-perfect Rust reencoder now runs in browsers, Cloudflare Workers,
Vercel Edge, and Node through WebAssembly.

```bash
BENCH_ITERATIONS=31 BENCH_REENCODE_ONLY=1 \
WTFGIF_BENCH_BACKEND=wasm npm run bench
```

| Fixture | wtfgif faster than omggif |
| --- | ---: |
| 18d | 8.19x |
| Clap | 1.23x |
| Homer | 14.68x |
| Chip | 17.70x |
| GIG | 15.44x |
| NOD | 8.60x |
| Proud | 20.00x |
| cat | 14.42x |
| excuse | 9.60x |
| blob | 9.74x |
| parrot | 3.38x |
| tenor | 16.00x |
| Geometric mean | 9.45x |

These are first-call numbers in 31 fresh Node processes per implementation and
fixture. They include Wasm's first-call cost and use no warmup. Browser and edge
support is separately exercised by real-browser, Cloudflare workerd, and Vercel
Edge VM smoke tests.

Page-load preparation improves fresh LZW recompression, but it does not remove
the actual LZW decode and encode work. Profiling found decoding responsible for
roughly 57–85% of its hot runtime and literal reencoding for roughly 15–43%.
The remux API avoids both stages by preserving the original compressed payload.

## Correctness gates

- Source and reencoded decoded pixels must match exactly for every timed sample.
- Source and remuxed decoded pixels must match exactly for every timed sample.
- A randomized parity stress run covered 200 generated GIFs across 4, 16, 64,
  128, and 256 colors, global palettes, and identical local palettes.
- Unit tests compare reader and writer behavior with omggif across the fixture
  corpus.

Record new results here only with the exact command, sample count, environment,
backend, and correctness gate.
