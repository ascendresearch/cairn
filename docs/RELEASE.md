# Release and cross-linking

Cairn publishes the controller and worker for both deployment architectures from one locked source
revision. Target machines do not need Rust, Cargo, a C compiler, or build-time network access.

## Frozen toolchain

The normative versions are recorded in [`release/toolchain.toml`](../release/toolchain.toml):

- Rust 1.85.0;
- cargo-zigbuild 0.21.8;
- Zig 0.14.1;
- `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`;
- GLIBC 2.28 maximum symbol baseline.

The GNU baseline is intentionally below both initial deployment hosts: the Ascend host reports
GLIBC 2.34 and the GB10 host reports GLIBC 2.39. Zig supplies the target libc headers and linker
view. This prevents bundled SQLite or AWS-LC from accidentally compiling against the build host's
GLIBC 2.43.

Install the exact external tools without modifying target machines:

```bash
cargo +1.85.0 install --locked cargo-zigbuild --version 0.21.8
python3 -m venv .venv-zig
.venv-zig/bin/pip install ziglang==0.14.1
export CARGO_ZIGBUILD_PYTHON_PATH="$PWD/.venv-zig/bin/python"
```

The Python environment is only one supported way to obtain the exact Zig binary. It is not part of
Cairn's runtime or release artifact.

## Build and inspect

Run from a clean worktree:

```bash
scripts/build-release.sh
```

The script refuses a dirty tree by default, uses `Cargo.lock`, remaps the source checkout path,
checks ELF machine type and interpreter, rejects unexpected shared libraries, verifies that the
highest referenced GLIBC symbol is exactly 2.28, and creates deterministic archives under
`target/release-bundles/`. Each archive includes:

- `cairn-server` and `cairn-worker`;
- strict controller and worker configuration examples;
- MIT license;
- typed build metadata;
- SHA-256 manifest.

`CAIRN_RELEASE_ALLOW_DIRTY=1` exists only for development validation. A bundle whose metadata says
`dirty: true` must not be published or deployed as a release.

## Deployment gate

Before a release is promoted:

1. build both targets twice from the same clean revision and compare archive SHA-256 values;
2. verify each archive's `SHA256SUMS` before copying files;
3. execute `cairn-worker` on one x86-64 Ascend host and one AArch64 GB10 host;
4. prove both outbound mTLS sessions become durable live workers on one controller;
5. restart each worker from the same local SQLite journal and prove a fresh connection identity
   without changing its stable worker identity;
6. retain build metadata, bundle digests, host ABI observations, and smoke-test logs as release
   evidence.
