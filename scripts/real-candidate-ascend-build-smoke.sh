#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: scripts/real-candidate-ascend-build-smoke.sh <controller.json> <sha256:image-id> <candidate-state-dir> <candidate-proposal-id>" >&2
  exit 2
fi

readonly CONTROLLER_CONFIG="$(realpath "$1")"
readonly IMAGE_ID="$2"
readonly CANDIDATE_STATE="$(realpath "$3")"
readonly PROPOSAL_ID="$4"

if [[ ! "$IMAGE_ID" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "image identity must be one full lowercase sha256 Docker image ID" >&2
  exit 2
fi

if [[ ! "$PROPOSAL_ID" =~ ^cairn:v1:sha256:migration\.candidate-collection-proposal\.v1:[0-9a-f]{64}$ ]]; then
  echo "proposal identity must be one typed Candidate proposal V1 content ID" >&2
  exit 2
fi

for path in content.db cas/objects/sha256; do
  if [[ ! -e "$CANDIDATE_STATE/$path" ]]; then
    echo "missing Candidate episode state path: $CANDIDATE_STATE/$path" >&2
    exit 2
  fi
done

CAIRN_REAL_CONTROLLER_CONFIG="$CONTROLLER_CONFIG" \
CAIRN_REAL_ASCEND_IMAGE_ID="$IMAGE_ID" \
CAIRN_REAL_CANDIDATE_STATE_DIR="$CANDIDATE_STATE" \
CAIRN_REAL_CANDIDATE_PROPOSAL_ID="$PROPOSAL_ID" \
  cargo test -p cairn-server --test real_ascend_build_worker \
    scheduled_exact_candidate_proposal_reaches_remote_ascend_build -- --ignored --nocapture
