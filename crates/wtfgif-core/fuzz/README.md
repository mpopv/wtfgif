# Fuzzing

The decode target feeds arbitrary bytes through metadata parsing, frame decoding, and compositing. The encode target generates bounded arbitrary RGBA animations, encodes them, then reparses and decodes the result.

Run bounded smoke checks with:

```sh
npm run fuzz:smoke
```

The smoke runner copies the repository's valid GIFs and encoder seed into a
temporary corpus, then deletes the generated mutations. Set `FUZZ_RUNS` to
change the default 500 runs per target.

For deeper persistent campaigns, run the targets directly from
`crates/wtfgif-core`; libFuzzer will retain useful cases in the supplied corpus:

```sh
cargo +nightly fuzz run decode ../../test/gifs -- -max_total_time=300
cargo +nightly fuzz run encode -- -max_total_time=300
```
