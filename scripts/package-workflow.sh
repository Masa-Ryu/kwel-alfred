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
# Alfred uses icon.png; keep the SVG as the only tracked icon source.
/usr/bin/qlmanage -t -s 512 -o "$STAGE_DIR" "$PROJECT_DIR/workflow/assets/kwel-light.svg" >/dev/null
python3 - "$STAGE_DIR/kwel-light.svg.png" "$STAGE_DIR/icon.png" <<'PY_ICON'
import sys
from pathlib import Path

rendered, destination = map(Path, sys.argv[1:])
data = rendered.read_bytes()
if data[:8] != b"\x89PNG\r\n\x1a\n":
    raise ValueError("The SVG renderer did not produce a PNG icon.")
# Keep pixels and color information, omitting renderer EXIF/text metadata.
private_metadata = {b"eXIf", b"tEXt", b"zTXt", b"iTXt", b"tIME"}
output = bytearray(data[:8])
offset = 8
while offset < len(data):
    if offset + 12 > len(data):
        raise ValueError("The generated icon is truncated.")
    length = int.from_bytes(data[offset:offset + 4], "big")
    end = offset + length + 12
    if end > len(data):
        raise ValueError("The generated icon is truncated.")
    if data[offset + 4:offset + 8] not in private_metadata:
        output.extend(data[offset:end])
    offset = end
destination.write_bytes(output)
rendered.unlink()
PY_ICON
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
