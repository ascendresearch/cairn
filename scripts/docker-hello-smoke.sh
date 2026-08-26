#!/usr/bin/env bash
set -euo pipefail

image_id=${1:-${CAIRN_DOCKER_IMAGE_ID:-}}
if [[ ! $image_id =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "usage: $0 <sha256:full-docker-image-id>" >&2
  exit 2
fi

CAIRN_DOCKER_IMAGE_ID=$image_id \
  cargo test -p cairn-worker --lib \
  docker::tests::real_docker_hello_world_is_replayable \
  -- --ignored --exact --nocapture
