# Contributing

Bug reports and focused pull requests are welcome.

## Setup

wtfgif requires Node 20.16 or newer and the stable Rust toolchain. Install
`wasm-pack`, then install dependencies:

```bash
cargo install wasm-pack --locked
npm ci
```

## Before opening a pull request

Run the complete release gate:

```bash
npm run check
```

Changes to decoding or encoding must include a regression test. Compatibility
changes should compare behavior with omggif when applicable. Performance
changes should report the command, fixture set, and whether the measurement
used a warm process, cold process, WebAssembly, or the experimental native
addon.

Encoder changes are accepted by what their GIFs decode to, not by their bytes.
Build the baseline, keep a copy of its `dist` directory, build the candidate,
and run:

```bash
EQUIVALENCE_BASELINE=path/to/baseline/dist npm run validate:equivalence
```

The default lossless gate requires every corpus fixture to decode to the same
composited frames, delays, and loop count. Bitstream, frame-layout, and speed
changes must pass it. Palette or mapping changes that may choose different
pixels use `EQUIVALENCE_GATE=quality` instead: shape, delays, and binary alpha
stay exact, and RGB PSNR and SSIM may fall by at most
`EQUIVALENCE_MAX_PSNR_LOSS` (default 0.05 dB) and `EQUIVALENCE_MAX_SSIM_LOSS`
(default 0.0005). The paired first-encode profiler,
`scripts/profile/first-quality-paired.mjs`, applies the same rule through
`PROFILE_GATE=pixels` (the default) or `PROFILE_GATE=quality`. Headline encoding claims must come from a clean
`npm run bench` receipt and report output size and quality beside latency.
Regenerate the committed charts with `npm run bench:charts` after accepting a
new receipt. Run `npm run bench:docs` to also update all current benchmark text
from the JSON receipts. The corpus chart must keep speed, output size, and every
encoder's RGB quality visible together. The speed-against-size chart must label
every point with its PSNR. The browser comparison chart must keep latency,
output size, PSNR, and alpha agreement visible together.

## Release checklist

The release branch is `main`. Before publishing, run `npm run check`, confirm
the package and lockfile versions match, push the tagged commit to `main`, and
publish the same version to npm. The initialized RGBA race and strict-cold RGBA
race are separate contracts; report both when performance changes affect
startup or allocation behavior.
