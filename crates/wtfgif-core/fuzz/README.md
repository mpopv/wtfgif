# Fuzzing

The decode target feeds arbitrary bytes through metadata parsing, frame decoding, and compositing. The encode target generates bounded arbitrary RGBA animations, encodes them, then checks that the compact output composites exactly like literal full frames. The round-trip target encodes arbitrary RGBA and indexed animations and checks the decoded result against the source: shape, delays, and loop count survive, alpha is exactly binary at the threshold, equal opaque colors decode to equal colors, images whose colors fit the palette decode exactly, and indexed and delta frames decode back to their indices.

Run bounded smoke checks with:

```sh
npm run fuzz:smoke
```

The smoke runner copies the repository's valid GIFs and the encoder seeds into
a temporary corpus, then deletes the generated mutations. Set `FUZZ_RUNS` to
change the default 500 runs per target, or `FUZZ_SECONDS` to run each target
for a fixed time instead. CI fuzzes each target for 30 seconds on pull
requests and 15 minutes in the weekly scheduled run.

For deeper persistent campaigns, run the targets directly from
`crates/wtfgif-core`; libFuzzer will retain useful cases in the supplied corpus:

```sh
cargo +nightly fuzz run decode ../../test/gifs -- -max_total_time=300
cargo +nightly fuzz run encode -- -max_total_time=300
cargo +nightly fuzz run roundtrip fuzz/corpus/roundtrip -- -max_total_time=300
```
