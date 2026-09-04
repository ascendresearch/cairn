#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: scripts/real-cuda-reduction-smoke.sh <server.json> <sha256:image-id> <cuda-reduction-v1/input>" >&2
  exit 2
fi

readonly CONTROLLER_CONFIG="$(realpath "$1")"
readonly IMAGE_ID="$2"
readonly FIXTURE_ROOT="$(realpath "$3")"

if [[ ! "$IMAGE_ID" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "image identity must be one full lowercase sha256 Docker image ID" >&2
  exit 2
fi

for path in \
  CMakeLists.txt \
  include/reduce_sum.h \
  src/reduce_sum_kernel.cu \
  src/reduce_sum_launch.cu \
  tests/reference_main.cpp; do
  if [[ ! -f "$FIXTURE_ROOT/$path" ]]; then
    echo "missing CUDA fixture file: $FIXTURE_ROOT/$path" >&2
    exit 2
  fi
done

CAIRN_REAL_CONTROLLER_CONFIG="$CONTROLLER_CONFIG" \
CAIRN_REAL_GPU_IMAGE_ID="$IMAGE_ID" \
CAIRN_REAL_CUDA_FIXTURE_ROOT="$FIXTURE_ROOT" \
  cargo test -p cairn-server --test real_gpu_worker \
    scheduled_cuda_reduction_builds_and_passes_release_corpus -- --ignored --nocapture
