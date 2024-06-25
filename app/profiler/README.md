# SGX Page Access Profiler

## Build instructions

Make sure you have installed a recent version of the Rust compiler and related tools (e.g. using [rustup](https://rustup.rs/)).

Use `cargo build --release` to build the profiler.

To build with TLBlur-specific features, such as simulating the defense, use `cargo build --release --features tlblur`.

## Usage guide

The profiler requires two binaries:

- An SGX enclave to profile
- A shared object that implements the following interface:

```c
void profiler_setup(int eid, int enclave_size, void *enclave_base);
void profiler_run(int eid, char **args);
```

The `profiler_run` function should enable single-stepping, ecall into the enclave and disable single-stepping.

See `./target/release/sgx_tracer --help` or `./target/release/sgx_tlblur_sim --help` for usage instructions.

### Example usage: libjpeg

Change to the `app/libjpeg` directory and build the required binaries with `make all`, then run the profiler with

```sh
sudo ../profiler/target/release/sgx_tracer --so ./profiler-libjpeg.so -e ./Enclave/encl.so --output trace_libjpeg.vcd
```

### TLBlur-specific instructions

TODO

## Documentation

Use `cargo doc --open` to generate and open documentation.
