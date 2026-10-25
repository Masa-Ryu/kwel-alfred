#!/bin/bash
set -euo pipefail
cd -- "$(dirname -- "$0")/.."

profile_id="${KWEL_RESULT_PROFILE_ID:-}"
open_ref="${1-}"
exec ./kwel-bridge open --profile-id "$profile_id" --ref "$open_ref"
