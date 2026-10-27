#!/bin/bash
set -euo pipefail
cd -- "$(dirname -- "$0")/.."

query="${1-}"
profile_id="${KWEL_PROFILE_ID:-}"
exec ./kwel-alfred search --profile-id "$profile_id" --query "$query" --limit 20 --format alfred
