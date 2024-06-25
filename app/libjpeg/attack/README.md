# Page fault attack on libjpeg

## Build instructions

Make sure you have installed a recent version of the Rust compiler and related tools (e.g. using [rustup](https://rustup.rs/)).

Run `make all` in the parent directory.

Use `cargo build --release` to build the attack code.

## Usage guide

To run the attack directly on an SGX enclave using page faults:

```sh
cargo run --release -- -o reconstruct.bmp -i ../img/birds.jpg --color enclave -e ../Enclave/encl.so
```

The attack can also be executed by using a profiler trace, or by using explicit ocalls (for debugging purposes). See `cargo run --release -- --help` for more information.

## Documentation

Use `cargo doc --open` to generate and open documentation.
