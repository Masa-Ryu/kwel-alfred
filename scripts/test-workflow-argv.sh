#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/kwel-alfred-argv.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
mkdir -p "$TEMP_DIR/workflow/scripts"
cp "$PROJECT_DIR/workflow/alfred/scripts/"*.sh "$TEMP_DIR/workflow/scripts/"
cat > "$TEMP_DIR/workflow/kwel-alfred" <<'MOCK'
#!/bin/bash
printf '%s\0' "$@" >> "$KWEL_CAPTURE"
if [[ "${1-}" == open ]]; then
  exit "${KWEL_MOCK_OPEN_STATUS:-0}"
fi
printf '%s\n' '{"skipknowledge":true,"items":[]}'
MOCK
chmod 755 "$TEMP_DIR/workflow/kwel-alfred" "$TEMP_DIR/workflow/scripts/"*.sh

SENTINEL="$TEMP_DIR/command-was-not-run"
QUERY="日本語 'single' \"double\" spaces & punctuation ; \$(touch $SENTINEL)"
# Synthetic UUID-shaped fixtures; no user profiles or credentials.
TEST_PROFILE_A="00000000-0000-4000-8000-00000000000a"
TEST_PROFILE_B="00000000-0000-4000-8000-00000000000b"
TEST_OPEN_REF="00000000-0000-4000-8000-000000000101"
CAPTURE="$TEMP_DIR/argv.bin"

# A configuration change takes effect on the very next search. Empty or invalid
# configuration is forwarded unchanged so the CLI can reject it before setup.
for CONFIGURED_PROFILE_ID in "$TEST_PROFILE_A" "$TEST_PROFILE_B" "" "invalid-id"; do
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
for RESULT_PROFILE_ID in "$TEST_PROFILE_A" ""; do
  : > "$CAPTURE"
  KWEL_CAPTURE="$CAPTURE" KWEL_PROFILE_ID="$TEST_PROFILE_B" KWEL_RESULT_PROFILE_ID="$RESULT_PROFILE_ID" bash "$TEMP_DIR/workflow/scripts/open-result.sh" "$TEST_OPEN_REF" >"$TEMP_DIR/output.json"
  python3 - "$CAPTURE" "$RESULT_PROFILE_ID" "$TEST_OPEN_REF" <<'PY'
import sys
from pathlib import Path
actual = Path(sys.argv[1]).read_bytes().split(b"\0")[:-1]
expected = [b"open", b"--profile-id", sys.argv[2].encode(), b"--ref", sys.argv[3].encode()]
assert actual == expected, (actual, expected)
PY
done

# Open failures emit one message for Alfred's conditional notification;
# successful opens emit nothing.
: > "$CAPTURE"
KWEL_CAPTURE="$CAPTURE" KWEL_RESULT_PROFILE_ID="$TEST_PROFILE_A" KWEL_MOCK_OPEN_STATUS=0 bash "$TEMP_DIR/workflow/scripts/open-result.sh" "$TEST_OPEN_REF" >"$TEMP_DIR/open-success.txt"
test ! -s "$TEMP_DIR/open-success.txt"
: > "$CAPTURE"
KWEL_CAPTURE="$CAPTURE" KWEL_RESULT_PROFILE_ID="$TEST_PROFILE_A" KWEL_MOCK_OPEN_STATUS=1 bash "$TEMP_DIR/workflow/scripts/open-result.sh" "$TEST_OPEN_REF" >"$TEMP_DIR/open-failure.txt"
test "$(cat "$TEMP_DIR/open-failure.txt")" = 'Could not open this result. Search again.'

test ! -e "$SENTINEL"
printf '%s\n' 'Configured Profile ID changes, result-bound profile opening, and Unicode/shell-sensitive argv are preserved safely.'
