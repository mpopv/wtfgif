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
changes should preserve decoded pixels and report the command, fixture set, and
whether the measurement used a warm process, cold process, WebAssembly, or the
experimental native addon. Headline encoding claims must come from a clean
`npm run bench` receipt and report output size and quality beside latency.
Regenerate the committed charts with `npm run bench:charts` after accepting a
new receipt. Run `npm run bench:docs` to also update all current benchmark text
from the JSON receipts. The corpus chart must keep speed, output size, and both
encoders' RGB quality visible together. The browser comparison chart must keep
latency, output size, PSNR, and alpha agreement visible together.

## Release checklist

The release branch is `main`. Before publishing, run `npm run check`, confirm
the package and lockfile versions match, push the tagged commit to `main`, and
publish the same version to npm. The initialized RGBA race and strict-cold RGBA
race are separate contracts; report both when performance changes affect
startup or allocation behavior.
