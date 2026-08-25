# Release and cross-linking

Cairn publishes the controller and worker for both deployment architectures from one locked source
revision. Target machines do not need Rust, Cargo, a C compiler, or build-time network access.

## Frozen toolchain

The normative versions are recorded in [`release/toolchain.toml`](../release/toolchain.toml):

- Rust 1.85.0;
- cargo-zigbuild 0.21.8;
- Zig 0.14.1;
- uv 0.12.5 with Python 3.12 for the build-only Zig environment;
- `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`;
- GLIBC 2.28 maximum symbol baseline.

The GNU baseline is intentionally below both initial deployment hosts: the Ascend host reports
GLIBC 2.34 and the GB10 host reports GLIBC 2.39. Zig supplies the target libc headers and linker
view. This prevents bundled SQLite or AWS-LC from accidentally compiling against the build host's
GLIBC 2.43.

Install the exact external tools without modifying target machines:

```bash
cargo +1.85.0 install --locked cargo-zigbuild --version 0.21.8
uv venv --python 3.12 .venv-zig
uv pip install --python .venv-zig/bin/python --requirement release/zig-requirements.txt
export CARGO_ZIGBUILD_PYTHON_PATH="$PWD/.venv-zig/bin/python"
```

The Python environment contains only the package pinned in `release/zig-requirements.txt` and is
used to expose the exact Zig executable to cargo-zigbuild. Python, uv, and that environment are not
part of Cairn's runtime or release artifact.

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

## GitHub automation

Pull requests and pushes to `main` run [the CI workflow](../.github/workflows/ci.yml). It executes
the complete Rust quality gate and builds and verifies both release architectures, so the packaging
path is exercised before a tag exists. Its bundles are retained briefly as workflow artifacts.

Pushing a semantic-version tag such as `v0.1.0` runs
[the release workflow](../.github/workflows/release.yml). It repeats the quality gate, builds both
architectures twice in independent Cargo target directories, compares the resulting archives and
checksums byte-for-byte, and only then passes the artifacts to a separate job with `contents: write`
permission. That final job verifies the downloaded checksums and creates the GitHub Release. The
toolchain setup is shared by both workflows in
[the local release action](../.github/actions/setup-release-toolchain/action.yml); uv, Python, and
the Python package are all installed explicitly rather than inherited from the hosted runner.

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
