#!/bin/bash
set -euo pipefail
cd -- "$(dirname -- "$0")/.."

profile_id="${KWEL_RESULT_PROFILE_ID:-}"
open_ref="${1-}"

if ./kwel-alfred open --profile-id "$profile_id" --ref "$open_ref" >/dev/null; then
	exit 0
fi

printf '%s\n' 'Could not open this result. Search again.'
