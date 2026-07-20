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

## Correctness gates

- Source and reencoded decoded pixels must match exactly for every timed sample.
- A randomized parity stress run covered 200 generated GIFs across 4, 16, 64,
  128, and 256 colors, global palettes, and identical local palettes.
- Unit tests compare reader and writer behavior with omggif across the fixture
  corpus.

Record new results here only with the exact command, sample count, environment,
backend, and correctness gate.
