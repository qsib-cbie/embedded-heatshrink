# Fuzzing

This directory uses `cargo-fuzz` to generate random data to run through the roundtrip encode/decode. It builds with instrumentation profiling configurations to keep track of code coverage. It changes its inputs of by randomly augmenting data from a corpus and tracking which lines of code are executed.

For example, the first usage of this tool found some dead code in the encoder and a lot of dead code in the decoder.

You will need llvm-tools-preview and nightly

* Clean: `cargo +nightly clean`
* Run fuzzing for hours: `RUSTFLAGS="-C instrument-coverage -Z profile" cargo +nightly fuzz run -j $(nproc) fuzz_target_1 -- -max_len=128000000`
* Merge the profile coverage artifacts: `cargo +nightly fuzz coverage fuzz_target_1`
* Generate an HTML file highlighting code with coverage: `cargo +nightly cov -- show fuzz/target/aarch64-apple-darwin/release/fuzz_target_1 --format=html --instr-profile=fuzz/coverage/fuzz_target_1/coverage.profdata > index.html`