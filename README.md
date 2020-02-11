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

Note: The latest version of `cargo-xbuild` supports all nightlies after 2019-02-04. If you are on an older nightly, you need to install version 0.5.4: `cargo install cargo-xbuild --version 0.5.4`.

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
panic_immediate_abort = false
```

- The `memcpy` flag defines whether the `mem` feature of the `compiler_builtins` crate should be activated. Turning this flag off allows to specify own versions of the `memcpy`, `memset` etc. functions.
- The `sysroot_path` flag specifies the directory where the sysroot should be placed.
- The `panic_immediate_abort` flag specifies whether the `panic_immediate_abort` feature the of `core` crate should be defined.

### Environment Variables

In addition to the above configuration keys, `cargo-xbuild` can be also configured through the following environment variables:

- The `XBUILD_SYSROOT_PATH` variable can be used to specify where `cargo-xbuild` should place the generated sysroot. This variables takes precendence over the `package.metadata.cargo-xbuild.sysroot_path` configuration key.

## Dev channel

If you want to use a local Rust source instead of `rust-src` rustup component, you can set the `XARGO_RUST_SRC` environment variable.

```
# The source of the `core` crate must be in `$XARGO_RUST_SRC/libcore`
$ export XARGO_RUST_SRC=/path/to/rust/src

$ cargo xbuild --target msp430-none-elf.json
```

## Using on Android

It's possible to run cargo-xbuild on your Android phone:

### Install Termux and Nightly Rustc

- Install [termux](https://play.google.com/store/apps/details?id=com.termux)
- Install fish shell and set as default (optional): `pkg install fish; chsh -s fish; fish`
- Install some basic tools: `pkg install wget tar`
- Add the [community repository by its-pointless](https://wiki.termux.com/wiki/Package_Management#By_its-pointless_.28live_the_dream.29:): `wget https://its-pointless.github.io/setup-pointless-repo.sh; bash setup-pointless-repo.sh`
- Install rust nightly: `pkg install rustc cargo rustc-nightly`
- Prepend the nightly rustc path to your `$PATH` in order to use nightly (fish syntax): `set -U fish_user_paths $PREFIX/opt/rust-nightly/bin/ $fish_user_paths`
- `rustc --version` should now return a nightly version

### (Optional) Install Git and Clone your Repository

- Install git: `pkg install git`
- Clone a repository of your choice: `git clone https://github.com/phil-opp/blog_os.git`

### Install Xbuild

- Install cargo-xbuild: `cargo install cargo-xbuild`
- Add the cargo bin directory to your `$PATH` (fish syntax): `set -U fish_user_paths ~/.cargo/bin/ $fish_user_paths`
- Now `cargo xbuild` should be available.

It does not work yet because it needs access to the rust source code. By default it tries to use rustup for this, but we have no rustup support so we need a different way.

### Providing the Rust Source Code

The Rust source code corresponding to our installed nightly is available in the `its-pointless` repository:

- Download it: `wget https://github.com/its-pointless/its-pointless.github.io/raw/master/rust-src-nightly.tar.xz`
- Extract it: `tar xf rust-src-nightly.tar.xz`
- Set the `XARGO_RUST_SRC` environment variable to tell cargo-xbuild the source path (fish syntax): `set -Ux XARGO_RUST_SRC ~/rust-src-nightly/rust-src/lib/rustlib/src/rust/src`

Now cargo-xbuild should no longer complain about a missing `rust-src` component. However it will throw an I/O error after building the sysroot. The problem is that the downloaded Rust source code has a different structure than the source provided by rustup. We can fix this by adding a symbolic link:

```
ln -s ~/../usr/opt/rust-nightly/bin ~/../usr/opt/rust-nightly/lib/rustlib/aarch64-linux-android/bin
```

Now `cargo xbuild --target your-target.json` should work!

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
