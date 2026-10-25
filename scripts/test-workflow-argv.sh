#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/kwel-alfred-argv.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
mkdir -p "$TEMP_DIR/workflow/scripts"
cp "$PROJECT_DIR/workflow/scripts/"*.sh "$TEMP_DIR/workflow/scripts/"
cat > "$TEMP_DIR/workflow/kwel-bridge" <<'MOCK'
#!/bin/bash
printf '%s\0' "$@" >> "$KWEL_CAPTURE"
printf '%s\n' '{"skipknowledge":true,"items":[]}'
MOCK
chmod 755 "$TEMP_DIR/workflow/kwel-bridge" "$TEMP_DIR/workflow/scripts/"*.sh

SENTINEL="$TEMP_DIR/command-was-not-run"
QUERY="日本語 'single' \"double\" spaces & punctuation ; \$(touch $SENTINEL)"
PROFILE_A="550e8400-e29b-41d4-a716-446655440000"
PROFILE_B="550e8400-e29b-41d4-a716-446655440001"
OPEN_REF="111e8400-e29b-41d4-a716-446655440001"
CAPTURE="$TEMP_DIR/argv.bin"

# A configuration change takes effect on the very next search. Empty or invalid
# configuration is forwarded unchanged so the CLI can reject it before setup.
for CONFIGURED_PROFILE_ID in "$PROFILE_A" "$PROFILE_B" "" "invalid-id"; do
  : > "$CAPTURE"
  KWEL_CAPTURE="$CAPTURE" KWEL_PROFILE_ID="$CONFIGURED_PROFILE_ID" bash "$TEMP_DIR/workflow/scripts/search.sh" "$QUERY" >"$TEMP_DIR/output.json"
  python3 - "$CAPTURE" "$QUERY" "$CONFIGURED_PROFILE_ID" <<'PY'
import sys
from pathlib import Path
actual = Path(sys.argv[1]).read_bytes().split(b"\0")[:-1]
expected = [b"search", b"--profile-id", sys.argv[3].encode(), b"--query", sys.argv[2].encode(), b"--limit", b"20", b"--format", b"alfred"]
assert actual == expected, (actual, expected)
PY
done

# A result found in A must still open in A after configuration changes to B.
# A missing result binding must never fall back to the currently configured B.
for RESULT_PROFILE_ID in "$PROFILE_A" ""; do
  : > "$CAPTURE"
  KWEL_CAPTURE="$CAPTURE" KWEL_PROFILE_ID="$PROFILE_B" KWEL_RESULT_PROFILE_ID="$RESULT_PROFILE_ID" bash "$TEMP_DIR/workflow/scripts/open-result.sh" "$OPEN_REF" >"$TEMP_DIR/output.json"
  python3 - "$CAPTURE" "$RESULT_PROFILE_ID" "$OPEN_REF" <<'PY'
import sys
from pathlib import Path
actual = Path(sys.argv[1]).read_bytes().split(b"\0")[:-1]
expected = [b"open", b"--profile-id", sys.argv[2].encode(), b"--ref", sys.argv[3].encode()]
assert actual == expected, (actual, expected)
PY
done

test ! -e "$SENTINEL"
printf '%s\n' 'Configured Profile ID changes, result-bound profile opening, and Japanese/shell-sensitive argv are preserved safely.'
