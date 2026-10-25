#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
PACKAGE_PATH="${1:-$PROJECT_DIR/dist/Kwel.alfredworkflow}"
CONFIG_FILE="$PROJECT_DIR/config/distribution.json"

if [[ ! -f "$PACKAGE_PATH" ]]; then
  printf 'Workflow package not found: %s\n' "$PACKAGE_PATH" >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/kwel-alfred-verify.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
python3 - "$PACKAGE_PATH" "$TEMP_DIR" <<'PY_ARCHIVE'
import stat
import sys
import zipfile
from pathlib import Path

root = Path(sys.argv[2])
executables = {'kwel-bridge', 'scripts/search.sh', 'scripts/open-result.sh'}
expected = executables | {'info.plist', 'icon.png', 'README.md', 'LICENSE'}
with zipfile.ZipFile(sys.argv[1]) as archive:
    names = archive.namelist()
    assert len(names) == len(set(names)), 'Duplicate archive entries are forbidden.'
    assert set(names) == expected, f'Unexpected package contents: {set(names) ^ expected}'
    for entry in archive.infolist():
        mode = entry.external_attr >> 16
        expected_mode = 0o755 if entry.filename in executables else 0o644
        assert entry.create_system == 3 and stat.S_ISREG(mode), f'Not a regular Unix file: {entry.filename}'
        assert stat.S_IMODE(mode) == expected_mode, f'Unsafe package permissions: {entry.filename}'
        destination = root / entry.filename
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(archive.read(entry))  # Also verifies the ZIP CRC.
        destination.chmod(expected_mode)
PY_ARCHIVE
LLVM_LIPO="${LLVM_LIPO:-llvm-lipo}"
"$LLVM_LIPO" "$TEMP_DIR/kwel-bridge" -verify_arch arm64 x86_64

python3 - "$CONFIG_FILE" "$TEMP_DIR/info.plist" "$TEMP_DIR" <<'PY'
import json
import plistlib
import re
import sys
from pathlib import Path

config = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
plist_path = Path(sys.argv[2])
root = Path(sys.argv[3])
info = plistlib.loads(plist_path.read_bytes())
assert info["bundleid"] == "com.kwel.alfred.search"
assert info["version"] == config["version"]
assert not list(root.rglob("prefs.plist")), "Personal configuration must not be packaged."
assert not list(root.rglob("profile-selection.json")), "Saved profile selection must not be packaged."

fields = info["userconfigurationconfig"]
assert len(fields) == 1
field = fields[0]
assert field["label"] == "Kwel Profile ID"
assert field["variable"] == "KWEL_PROFILE_ID"
assert field["type"] == "textfield"
assert field["description"] == "Paste the Profile ID from Kwel Settings → Alfred Integration."
assert field["config"]["default"] == ""
assert field["config"]["placeholder"] == ""
assert field["config"]["required"] is True
assert field["config"]["trim"] is True
assert not info.get("variables", {}).get("KWEL_PROFILE_ID"), "Do not ship a configured Profile ID."

objects = info["objects"]
assert len(objects) == 4
assert {obj["type"] for obj in objects} == {
    "alfred.workflow.trigger.fallback",
    "alfred.workflow.input.scriptfilter",
    "alfred.workflow.action.script",
    "alfred.workflow.output.notification",
}
fallback = next(obj for obj in objects if obj["type"] == "alfred.workflow.trigger.fallback")
search = next(obj for obj in objects if obj["type"] == "alfred.workflow.input.scriptfilter")
open_action = next(obj for obj in objects if obj["type"] == "alfred.workflow.action.script")
notification = next(obj for obj in objects if obj["type"] == "alfred.workflow.output.notification")
assert fallback["config"]["text"] == "Search Kwel"
assert search["config"].get("keyword", "") == ""
assert search["config"]["alfredfiltersresults"] is False
assert search["config"]["argumenttype"] == 1
assert search["config"]["scriptargtype"] == 1  # Query is passed through argv.
assert "inboundconfig" not in search["config"]  # Alfred's default input mode is Direct.
assert search["config"]["queuemode"] == 2  # Terminate the older query's process.
assert search["config"]["scriptfile"] == "./scripts/search.sh"
assert search["config"]["type"] == 8  # Execute the external file, not the empty inline script.
assert open_action["config"]["scriptfile"] == "./scripts/open-result.sh"
assert open_action["config"]["type"] == 8
assert open_action["config"]["scriptargtype"] == 1  # Selected openRef is passed through argv.
assert notification["config"]["text"] == "Could not open this result. Search again."
assert notification["config"]["onlyshowifquerypopulated"] is True
for forbidden in ["repair", "doctor", "uninstall", "status", "profile-chooser", "select-profile", "change-profile", "discover"]:
    assert forbidden not in str(objects).lower(), forbidden

connections = info["connections"]
assert set(connections) == {fallback["uid"], search["uid"], open_action["uid"]}
for source, destination in [(fallback, search), (search, open_action), (open_action, notification)]:
    edges = connections[source["uid"]]
    assert len(edges) == 1
    assert edges[0]["destinationuid"] == destination["uid"]
    assert edges[0].get("modifiers", 0) == 0
    assert not edges[0].get("sourceoutputuid")

script_files = {"search.sh", "open-result.sh"}
assert {path.name for path in (root / "scripts").iterdir()} == script_files
for name in script_files:
    path = root / "scripts" / name
    assert path.is_file() and path.stat().st_mode & 0o111
search_script = (root / "scripts/search.sh").read_text(encoding="utf-8")
open_script = (root / "scripts/open-result.sh").read_text(encoding="utf-8")
assert 'profile_id="${KWEL_PROFILE_ID:-}"' in search_script
assert 'exec ./kwel-bridge search --profile-id "$profile_id" --query "$query" --limit 20 --format alfred' in search_script
assert 'profile_id="${KWEL_RESULT_PROFILE_ID:-}"' in open_script
assert 'open_ref="${1-}"' in open_script
assert './kwel-bridge open --profile-id "$profile_id" --ref "$open_ref" >/dev/null' in open_script
assert "Could not open this result. Search again." in open_script
assert "KWEL_PROFILE_ID" not in open_script
assert all((root / name).is_file() for name in ["kwel-bridge", "icon.png", "README.md", "LICENSE"])
icon = (root / "icon.png").read_bytes()
assert icon[:8] == b"\x89PNG\r\n\x1a\n"
assert int.from_bytes(icon[16:20], "big") == 512
assert int.from_bytes(icon[20:24], "big") == 512
offset = 8
while offset < len(icon):
    length = int.from_bytes(icon[offset:offset + 4], "big")
    assert offset + length + 12 <= len(icon)
    assert icon[offset + 4:offset + 8] not in {b"eXIf", b"tEXt", b"zTXt", b"iTXt", b"tIME"}
    offset += length + 12
assert re.fullmatch(r"[a-p]{32}", config["chromeExtensionId"])
binary = (root / "kwel-bridge").read_bytes()
for protocol_string in [config["chromeExtensionId"], "com.kwel.alfred", "profileId", "open-page", "openRef", "KWEL_RESULT_PROFILE_ID", "com.google.Chrome"]:
    assert protocol_string.encode() in binary, protocol_string

print("Required Profile ID configuration, direct Fallback search/open graph, scripts, protocol, and universal binary slices are valid.")
PY
