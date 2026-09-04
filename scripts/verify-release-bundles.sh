#!/usr/bin/env bash
set -euo pipefail

for tool in tar sha256sum git; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "release bundle verification cannot run: $tool is unavailable" >&2
    exit 2
  }
done

readonly BUNDLE_ROOT="${1:-target/release-bundles}"
readonly COMMIT_SHORT="$(git rev-parse --short=12 HEAD)"
readonly TARGETS=(
  aarch64-unknown-linux-gnu
  x86_64-unknown-linux-gnu
  x86_64-unknown-linux-musl
)

if [[ ! -d "$BUNDLE_ROOT" ]]; then
  echo "release bundle directory does not exist: $BUNDLE_ROOT" >&2
  exit 2
fi

for target in "${TARGETS[@]}"; do
  bundle="cairn-$COMMIT_SHORT-$target.tar.gz"
  checksum="$bundle.sha256"
  if [[ ! -f "$BUNDLE_ROOT/$bundle" || ! -f "$BUNDLE_ROOT/$checksum" ]]; then
    echo "missing release bundle or checksum for $target" >&2
    exit 1
  fi

  for required in \
    ./BUILD-METADATA.json \
    ./LICENSE \
    ./SHA256SUMS \
    ./bin/cairn-server \
    ./bin/cairn-worker \
    ./config/server.example.json \
    ./config/worker.example.json; do
    if ! tar -tzf "$BUNDLE_ROOT/$bundle" "$required" >/dev/null; then
      echo "$bundle does not contain $required" >&2
      exit 1
    fi
  done
done

(
  cd "$BUNDLE_ROOT"
  for target in "${TARGETS[@]}"; do
    sha256sum --check --strict "cairn-$COMMIT_SHORT-$target.tar.gz.sha256"
  done
)

echo "verified ${#TARGETS[@]} release bundles in $BUNDLE_ROOT"
