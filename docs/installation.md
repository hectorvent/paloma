# Installation

`paloma` is a single static binary: no Node, Python or JVM at runtime.

## Release archives

Each release attaches archives for macOS, Linux and Windows on x86_64 and arm64
(`paloma-vX.Y.Z-<target>.tar.gz` / `.zip`) plus SHA-256 checksums. Linux builds are
static (musl). Unpack, put `paloma` on your `PATH`, and you're done:

```bash
tar xzf paloma-v0.1.0-aarch64-apple-darwin.tar.gz
install -m 0755 paloma ~/.local/bin/
paloma version
```

Archives also carry man pages and shell completions under `dist-assets/`.

## From source

Needs a Rust toolchain (stable; `rust-toolchain.toml` pins the version used in CI).

```bash
cargo install --path crates/paloma-cli          # installs `paloma` to ~/.cargo/bin
# or
cargo build --release && cp target/release/paloma ~/.local/bin/
```

| Command | Produces |
|---------|----------|
| `cargo build` | Debug binary at `target/debug/paloma` (fast to compile) |
| `cargo build --release` | Optimized binary at `target/release/paloma` (LTO, stripped) |
| `cargo build --profile dist` | Size-optimized binary (`opt-level = "s"`) |
| `cargo xtask dist-assets` | Man pages and completions in `target/dist-assets/` |

TLS is rustls with the ring provider, so there is no OpenSSL dependency and
cross-compiling (e.g. with `cross`) needs no system libraries.

## Shell completion

```bash
paloma completion zsh > "${fpath[1]}/_paloma"
```

See [completion](commands/completion.md) for other shells.
