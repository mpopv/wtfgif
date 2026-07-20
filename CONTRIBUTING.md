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
experimental native addon.
