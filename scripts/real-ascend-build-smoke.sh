#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: scripts/real-ascend-build-smoke.sh <controller.json> <sha256:image-id> <ascend-add-v1>" >&2
  exit 2
fi

readonly CONTROLLER_CONFIG="$(realpath "$1")"
readonly IMAGE_ID="$2"
readonly FIXTURE_ROOT="$(realpath "$3")"

if [[ ! "$IMAGE_ID" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "image identity must be one full lowercase sha256 Docker image ID" >&2
  exit 2
fi

for path in add_custom.cpp image/harness/project/add_custom_tiling.h; do
  if [[ ! -f "$FIXTURE_ROOT/$path" ]]; then
    echo "missing Ascend fixture file: $FIXTURE_ROOT/$path" >&2
    exit 2
  fi
done

CAIRN_REAL_CONTROLLER_CONFIG="$CONTROLLER_CONFIG" \
CAIRN_REAL_ASCEND_IMAGE_ID="$IMAGE_ID" \
CAIRN_REAL_ASCEND_FIXTURE_ROOT="$FIXTURE_ROOT" \
  cargo test -p cairn-server --test real_ascend_build_worker -- --ignored --nocapture
