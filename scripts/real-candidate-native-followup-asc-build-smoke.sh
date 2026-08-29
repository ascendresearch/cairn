#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: scripts/real-candidate-native-followup-asc-build-smoke.sh <controller.json> <sha256:image-id> <candidate-followup-state-dir> <candidate-followup-id>" >&2
  exit 2
fi

readonly CONTROLLER_CONFIG="$(realpath "$1")"
readonly IMAGE_ID="$2"
readonly FOLLOWUP_STATE="$(realpath "$3")"
readonly FOLLOWUP_ID="$4"

if [[ ! "$IMAGE_ID" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "image identity must be one full lowercase sha256 Docker image ID" >&2
  exit 2
fi

if [[ ! "$FOLLOWUP_ID" =~ ^cairn:v1:sha256:migration\.candidate-native-followup-revision\.v1:[0-9a-f]{64}$ ]]; then
  echo "follow-up identity must be one typed Candidate native-followup V1 content ID" >&2
  exit 2
fi

for path in content.db cas/objects/sha256; do
  if [[ ! -e "$FOLLOWUP_STATE/$path" ]]; then
    echo "missing Candidate native follow-up episode state path: $FOLLOWUP_STATE/$path" >&2
    exit 2
  fi
done

CAIRN_REAL_CONTROLLER_CONFIG="$CONTROLLER_CONFIG" \
CAIRN_REAL_ASCEND_IMAGE_ID="$IMAGE_ID" \
CAIRN_REAL_CANDIDATE_NATIVE_FOLLOWUP_STATE_DIR="$FOLLOWUP_STATE" \
CAIRN_REAL_CANDIDATE_NATIVE_FOLLOWUP_ID="$FOLLOWUP_ID" \
  cargo test -p cairn-server --test real_ascend_build_worker \
    scheduled_exact_candidate_native_followup_reaches_product_owned_native_asc_gate -- --ignored --nocapture
