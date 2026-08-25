#!/usr/bin/env bash
set -euo pipefail

readonly EXPECTED_RUST="rustc 1.85.0 (4d91de4e4 2025-02-17)"
readonly EXPECTED_ZIGBUILD="cargo-zigbuild 0.21.8"
readonly EXPECTED_ZIG="0.14.1"
readonly GLIBC_BASELINE="2.28"
readonly SOURCE_ROOT="$(pwd -P)"
readonly OUTPUT_ROOT="${CAIRN_RELEASE_OUTPUT_DIR:-$SOURCE_ROOT/target/release-bundles}"
readonly BUILD_ROOT="${CAIRN_RELEASE_TARGET_DIR:-$SOURCE_ROOT/target/release-build}"
readonly ZIGBUILD_CACHE="${CAIRN_ZIGBUILD_CACHE_DIR:-$SOURCE_ROOT/target/release-tool-cache/cargo-zigbuild}"
readonly ZIG_GLOBAL_CACHE="${CAIRN_ZIG_GLOBAL_CACHE_DIR:-$SOURCE_ROOT/target/release-tool-cache/zig-global}"
readonly ZIG_LOCAL_CACHE="${CAIRN_ZIG_LOCAL_CACHE_DIR:-$SOURCE_ROOT/target/release-tool-cache/zig-local}"

if [[ ! -f "$SOURCE_ROOT/Cargo.lock" || ! -f "$SOURCE_ROOT/release/toolchain.toml" ]]; then
  echo "build-release must run from the Cairn repository root" >&2
  exit 2
fi

if [[ "${CAIRN_RELEASE_ALLOW_DIRTY:-0}" != "1" ]] && [[ -n "$(git status --porcelain)" ]]; then
  echo "release build refuses a dirty worktree; commit changes or set CAIRN_RELEASE_ALLOW_DIRTY=1 for a non-publishable validation" >&2
  exit 2
fi

if [[ "$(rustc --version)" != "$EXPECTED_RUST" ]]; then
  echo "expected $EXPECTED_RUST, observed $(rustc --version)" >&2
  exit 2
fi
if [[ "$(cargo-zigbuild --version)" != "$EXPECTED_ZIGBUILD" ]]; then
  echo "expected $EXPECTED_ZIGBUILD, observed $(cargo-zigbuild --version)" >&2
  exit 2
fi

zig_version() {
  if command -v zig >/dev/null 2>&1; then
    zig version
  elif [[ -n "${CARGO_ZIGBUILD_PYTHON_PATH:-}" ]]; then
    "$CARGO_ZIGBUILD_PYTHON_PATH" -m ziglang version
  else
    echo "zig is unavailable; install Zig or set CARGO_ZIGBUILD_PYTHON_PATH" >&2
    return 2
  fi
}

if [[ "$(zig_version)" != "$EXPECTED_ZIG" ]]; then
  echo "expected Zig $EXPECTED_ZIG, observed $(zig_version)" >&2
  exit 2
fi

readonly COMMIT="$(git rev-parse HEAD)"
readonly COMMIT_SHORT="${COMMIT:0:12}"
readonly SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}"
readonly DIRTY="$(if [[ -n "$(git status --porcelain)" ]]; then echo true; else echo false; fi)"

mkdir -p "$OUTPUT_ROOT" "$BUILD_ROOT" "$ZIGBUILD_CACHE" "$ZIG_GLOBAL_CACHE" "$ZIG_LOCAL_CACHE"

export CARGO_ZIGBUILD_CACHE_DIR="$ZIGBUILD_CACHE"
export ZIG_GLOBAL_CACHE_DIR="$ZIG_GLOBAL_CACHE"
export ZIG_LOCAL_CACHE_DIR="$ZIG_LOCAL_CACHE"
export SOURCE_DATE_EPOCH
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$SOURCE_ROOT=/src/cairn -C strip=symbols"

if (($# == 0)); then
  targets=(aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu)
else
  targets=("$@")
fi
for target in "${targets[@]}"; do
  case "$target" in
    aarch64-unknown-linux-gnu)
      expected_machine="AArch64"
      expected_interpreter="/lib/ld-linux-aarch64.so.1"
      ;;
    x86_64-unknown-linux-gnu)
      expected_machine="Advanced Micro Devices X86-64"
      expected_interpreter="/lib64/ld-linux-x86-64.so.2"
      ;;
    *)
      echo "unsupported release target: $target" >&2
      exit 2
      ;;
  esac

  cargo zigbuild \
    --locked \
    --release \
    --target "$target.$GLIBC_BASELINE" \
    --target-dir "$BUILD_ROOT" \
    -p cairn-server \
    -p cairn-worker

  stage="$OUTPUT_ROOT/.stage-$target"
  bundle="$OUTPUT_ROOT/cairn-$COMMIT_SHORT-$target.tar.gz"
  rm -rf "$stage"
  mkdir -p "$stage/bin" "$stage/config"
  cp "$BUILD_ROOT/$target/release/cairn-server" "$stage/bin/"
  cp "$BUILD_ROOT/$target/release/cairn-worker" "$stage/bin/"
  cp "$SOURCE_ROOT/config/controller.example.json" "$stage/config/"
  cp "$SOURCE_ROOT/config/worker.example.json" "$stage/config/"
  cp "$SOURCE_ROOT/LICENSE" "$stage/"

  for binary in cairn-server cairn-worker; do
    binary_path="$stage/bin/$binary"
    machine="$(readelf -h "$binary_path" | sed -n 's/^[[:space:]]*Machine:[[:space:]]*//p')"
    interpreter="$(readelf -l "$binary_path" | sed -n 's/.*Requesting program interpreter: \([^]]*\).*/\1/p')"
    maximum_glibc="$(readelf -W --version-info "$binary_path" | sed -n 's/.*Name: GLIBC_\([^ ]*\).*/\1/p' | sort -V | tail -1)"
    if [[ "$machine" != "$expected_machine" ]]; then
      echo "$binary has machine '$machine', expected '$expected_machine'" >&2
      exit 1
    fi
    if [[ "$interpreter" != "$expected_interpreter" ]]; then
      echo "$binary has interpreter '$interpreter', expected '$expected_interpreter'" >&2
      exit 1
    fi
    if [[ "$maximum_glibc" != "$GLIBC_BASELINE" ]]; then
      echo "$binary requires GLIBC_$maximum_glibc, expected maximum GLIBC_$GLIBC_BASELINE" >&2
      exit 1
    fi
    while IFS= read -r needed; do
      case "$needed" in
        ld-linux-aarch64.so.1|ld-linux-x86-64.so.2|libc.so.6|libdl.so.2|libgcc_s.so.1|libm.so.6|libpthread.so.0|librt.so.1) ;;
        *)
          echo "$binary has unexpected dynamic dependency: $needed" >&2
          exit 1
          ;;
      esac
    done < <(readelf -d "$binary_path" | sed -n 's/.*Shared library: \[\([^]]*\)\].*/\1/p')
  done

  server_sha="$(sha256sum "$stage/bin/cairn-server" | cut -d ' ' -f 1)"
  worker_sha="$(sha256sum "$stage/bin/cairn-worker" | cut -d ' ' -f 1)"
  printf '%s\n' \
    '{' \
    '  "schema_version": 1,' \
    "  \"commit\": \"$COMMIT\"," \
    "  \"dirty\": $DIRTY," \
    "  \"source_date_epoch\": $SOURCE_DATE_EPOCH," \
    "  \"target\": \"$target\"," \
    "  \"glibc_baseline\": \"$GLIBC_BASELINE\"," \
    "  \"rust\": \"1.85.0\"," \
    "  \"cargo_zigbuild\": \"0.21.8\"," \
    "  \"zig\": \"0.14.1\"," \
    '  "binaries": {' \
    "    \"cairn-server\": \"sha256:$server_sha\"," \
    "    \"cairn-worker\": \"sha256:$worker_sha\"" \
    '  }' \
    '}' > "$stage/BUILD-METADATA.json"

  (
    cd "$stage"
    find . -type f ! -name SHA256SUMS -print0 \
      | sort -z \
      | xargs -0 sha256sum \
      > SHA256SUMS
  )
  tar \
    --sort=name \
    --mtime="@$SOURCE_DATE_EPOCH" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    --format=posix \
    --pax-option=delete=atime,delete=ctime \
    -C "$stage" \
    -cf - \
    . \
    | gzip -n > "$bundle"
  (
    cd "$OUTPUT_ROOT"
    sha256sum "$(basename "$bundle")" > "$(basename "$bundle").sha256"
  )
  rm -rf "$stage"
  echo "created $bundle"
done
