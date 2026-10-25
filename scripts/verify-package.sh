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
/usr/bin/unzip -q "$PACKAGE_PATH" -d "$TEMP_DIR"
/usr/bin/plutil -lint "$TEMP_DIR/info.plist"
/usr/bin/lipo -verify_arch arm64 "$TEMP_DIR/kwel-bridge"
/usr/bin/lipo -verify_arch x86_64 "$TEMP_DIR/kwel-bridge"

python3 - "$CONFIG_FILE" "$TEMP_DIR/info.plist" "$TEMP_DIR" <<'PY'
import json
import plistlib
import re
import subprocess
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
assert len(objects) == 3
assert {obj["type"] for obj in objects} == {
    "alfred.workflow.trigger.fallback",
    "alfred.workflow.input.scriptfilter",
    "alfred.workflow.action.script",
}
fallback = next(obj for obj in objects if obj["type"] == "alfred.workflow.trigger.fallback")
search = next(obj for obj in objects if obj["type"] == "alfred.workflow.input.scriptfilter")
open_action = next(obj for obj in objects if obj["type"] == "alfred.workflow.action.script")
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
for forbidden in ["repair", "doctor", "uninstall", "status", "profile-chooser", "select-profile", "change-profile", "discover"]:
    assert forbidden not in str(objects).lower(), forbidden

connections = info["connections"]
assert set(connections) == {fallback["uid"], search["uid"]}
for source, destination in [(fallback, search), (search, open_action)]:
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
assert 'exec ./kwel-bridge open --profile-id "$profile_id" --ref "$open_ref"' in open_script
assert "KWEL_PROFILE_ID" not in open_script
assert all((root / name).is_file() for name in ["kwel-bridge", "icon.png", "README.md", "LICENSE"])
assert re.fullmatch(r"[a-p]{32}", config["chromeExtensionId"])
binary_strings = subprocess.check_output(["/usr/bin/strings", str(root / "kwel-bridge")], text=True)
for protocol_string in [config["chromeExtensionId"], "com.kwel.alfred", "profileId", "open-page", "openRef", "KWEL_RESULT_PROFILE_ID", "com.google.Chrome"]:
    assert protocol_string in binary_strings, protocol_string

print("Required Profile ID configuration, direct Fallback search/open graph, scripts, protocol, and universal binary slices are valid.")
PY
