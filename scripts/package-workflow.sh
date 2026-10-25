#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
OUTPUT_PATH="${1:-$PROJECT_DIR/dist/Kwel.alfredworkflow}"
DIST_DIR="$PROJECT_DIR/dist"
STAGE_DIR="$DIST_DIR/workflow-stage"

"$PROJECT_DIR/scripts/build.sh"

/usr/bin/plutil -lint "$PROJECT_DIR/workflow/info.plist"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/scripts"
cp "$PROJECT_DIR/workflow/info.plist" "$STAGE_DIR/info.plist"
cp "$PROJECT_DIR/workflow/icon.png" "$STAGE_DIR/icon.png"
cp "$DIST_DIR/kwel-bridge" "$STAGE_DIR/kwel-bridge"
cp "$PROJECT_DIR/workflow/scripts/"*.sh "$STAGE_DIR/scripts/"
cp "$PROJECT_DIR/README.md" "$STAGE_DIR/README.md"
cp "$PROJECT_DIR/LICENSE" "$STAGE_DIR/LICENSE"
chmod 755 "$STAGE_DIR/kwel-bridge" "$STAGE_DIR/scripts/"*.sh

mkdir -p "$(dirname -- "$OUTPUT_PATH")"
rm -f "$OUTPUT_PATH"
(cd "$STAGE_DIR" && /usr/bin/zip -X -q -r "$OUTPUT_PATH" .)
/usr/bin/unzip -t "$OUTPUT_PATH" >/dev/null

printf 'Created %s\n' "$OUTPUT_PATH"
