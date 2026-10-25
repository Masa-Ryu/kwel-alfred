#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
OUTPUT_PATH="${1:-$PROJECT_DIR/dist/Kwel.alfredworkflow}"
DIST_DIR="$PROJECT_DIR/dist"
STAGE_DIR="$DIST_DIR/workflow-stage"
RESVG="${RESVG:-resvg}"

if [[ "$("$RESVG" --version)" != '0.45.1' ]]; then
  printf '%s\n' 'Packaging requires resvg 0.45.1.' >&2
  exit 1
fi
"$PROJECT_DIR/scripts/build.sh"
python3 - "$PROJECT_DIR/workflow/info.plist" <<'PY_PLIST'
import plistlib
import sys
from pathlib import Path

info = plistlib.loads(Path(sys.argv[1]).read_bytes())
assert isinstance(info, dict), 'Workflow info.plist must be a dictionary.'
PY_PLIST
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/scripts"
cp "$PROJECT_DIR/workflow/info.plist" "$STAGE_DIR/info.plist"
# Render the only tracked icon source without platform fonts or GUI services.
"$RESVG" --skip-system-fonts --width 512 --height 512 "$PROJECT_DIR/workflow/assets/kwel-light.svg" "$STAGE_DIR/icon.png"
python3 - "$STAGE_DIR/icon.png" <<'PY_ICON'
import sys
from pathlib import Path

path = Path(sys.argv[1])
data = path.read_bytes()
if data[:8] != b"\x89PNG\r\n\x1a\n":
    raise ValueError('The SVG renderer did not produce a PNG icon.')
private_metadata = {b'eXIf', b'tEXt', b'zTXt', b'iTXt', b'tIME'}
output = bytearray(data[:8])
offset = 8
while offset < len(data):
    if offset + 12 > len(data):
        raise ValueError('The generated icon is truncated.')
    length = int.from_bytes(data[offset:offset + 4], 'big')
    end = offset + length + 12
    if end > len(data):
        raise ValueError('The generated icon is truncated.')
    if data[offset + 4:offset + 8] not in private_metadata:
        output.extend(data[offset:end])
    offset = end
path.write_bytes(output)
PY_ICON
cp "$DIST_DIR/kwel-bridge" "$STAGE_DIR/kwel-bridge"
cp "$PROJECT_DIR/workflow/scripts/"*.sh "$STAGE_DIR/scripts/"
cp "$PROJECT_DIR/README.md" "$STAGE_DIR/README.md"
cp "$PROJECT_DIR/LICENSE" "$STAGE_DIR/LICENSE"
chmod 755 "$STAGE_DIR/kwel-bridge" "$STAGE_DIR/scripts/"*.sh

# Stable ordering, timestamps and explicit Unix permissions keep the archive
# independent of the host umask and source file modification times.
python3 - "$STAGE_DIR" "$OUTPUT_PATH" <<'PY_ZIP'
import stat
import sys
import zipfile
from pathlib import Path

stage, destination = map(Path, sys.argv[1:])
destination.parent.mkdir(parents=True, exist_ok=True)
executables = {'kwel-bridge', 'scripts/search.sh', 'scripts/open-result.sh'}
with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    for path in sorted(stage.rglob('*')):
        if path.is_dir():
            continue
        name = path.relative_to(stage).as_posix()
        info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
        info.create_system = 3
        info.external_attr = (stat.S_IFREG | (0o755 if name in executables else 0o644)) << 16
        info.compress_type = zipfile.ZIP_DEFLATED
        archive.writestr(info, path.read_bytes(), compresslevel=9)
PY_ZIP
printf 'Created %s\n' "$OUTPUT_PATH"
