# cargo-xbuild

Cargo-xbuild is a wrapper for `cargo build`, which cross compiles the sysroot crates `core`, `compiler_builtins`, and `alloc` for custom targets. It is a simplified fork of [`xargo`](https://github.com/japaric/xargo), which is in maintainance mode.

## Dependencies

- The `rust-src` component, which you can install with `rustup component add
  rust-src`.

- Rust and Cargo.

## Installation

```
$ cargo install cargo-xbuild
```

## Usage

Just use `cargo xbuild` instead of `cargo build` when cross-compiling for a custom target.

```
cargo xbuild --target your-target-name.json
```

Instead of the "can't find crate for `core`" error you would get with a plain `cargo build`, this crate cross-compiles the `core`, `compiler_builtins`, and `alloc` crates and then invokes `cargo build` with a modified sysroot. The sysroot is compiled in the `target` directory of your crate.

All additional arguments (e.g. `--release` or `--verbose`) are forwarded to `cargo build`.

## Configuration

To configure `cargo-xbuild` create a `package.metadata.cargo-xbuild` table in your `Cargo.toml`. The following options are available:

```toml
[package.metadata.cargo-xbuild]
memcpy = true
sysroot_path = "target/sysroot"
```

- The `memcpy` flag defines whether the `mem` feature of the `compiler_builtins` crate should be activated. Turning this flag off allows to specify own versions of the `memcpy`, `memset` etc. functions.
- The `sysroot_path` flag specifies the directory where the sysroot should be placed.

## Dev channel

If you want to use a local Rust source instead of `rust-src` rustup component, you can set the `XARGO_RUST_SRC` environment variable.

```
# The source of the `core` crate must be in `$XARGO_RUST_SRC/libcore`
$ export XARGO_RUST_SRC=/path/to/rust/src

$ cargo xbuild --target msp430-none-elf.json
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
