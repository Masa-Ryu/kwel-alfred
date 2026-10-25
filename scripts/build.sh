#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
DIST_DIR="$PROJECT_DIR/dist"
CONFIG_FILE="$PROJECT_DIR/config/distribution.json"

read_config() {
  python3 - "$CONFIG_FILE" "$1" <<'PY'
import json
import sys
from pathlib import Path

config = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
value = config[sys.argv[2]]
print(str(value).lower() if isinstance(value, bool) else value)
PY
}

EXTENSION_ID="$(read_config chromeExtensionId)"
VERSION="$(read_config version)"
CLIENT_SUPPORT="$(read_config kwelClientSupported)"

if [[ ! "$EXTENSION_ID" =~ ^[a-p]{32}$ ]]; then
  printf '%s\n' 'Build stopped: no valid published Kwel Chrome extension ID is configured.' >&2
  exit 1
fi

LLVM_LIPO="${LLVM_LIPO:-llvm-lipo}"
if ! command -v "$LLVM_LIPO" >/dev/null; then
  printf '%s\n' 'Build stopped: install llvm-lipo or set LLVM_LIPO to its path.' >&2
  exit 1
fi

mkdir -p "$DIST_DIR"
export GOCACHE="${GOCACHE:-${TMPDIR:-/tmp}/kwel-alfred-go-cache}"
mkdir -p "$GOCACHE"
cd "$PROJECT_DIR"

LDFLAGS="-s -w -X main.allowedExtensionID=$EXTENSION_ID -X main.version=$VERSION -X main.kwelClientSupport=$CLIENT_SUPPORT"
GOOS=darwin GOARCH=arm64 CGO_ENABLED=0 go build -trimpath -ldflags "$LDFLAGS" -o "$DIST_DIR/kwel-bridge-arm64" ./cmd/kwel-bridge
GOOS=darwin GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -ldflags "$LDFLAGS" -o "$DIST_DIR/kwel-bridge-amd64" ./cmd/kwel-bridge
"$LLVM_LIPO" -create "$DIST_DIR/kwel-bridge-arm64" "$DIST_DIR/kwel-bridge-amd64" -output "$DIST_DIR/kwel-bridge"
"$LLVM_LIPO" "$DIST_DIR/kwel-bridge" -verify_arch arm64 x86_64
chmod 755 "$DIST_DIR/kwel-bridge"

if [[ "${KWEL_RELEASE:-0}" == "1" && "$CLIENT_SUPPORT" != "true" ]]; then
  printf '%s\n' 'Release build stopped: the current Kwel extension source does not implement Native Messaging support.' >&2
  exit 1
fi

printf 'Built universal macOS Bridge (%s, %s) at %s\n' "$VERSION" "$CLIENT_SUPPORT" "$DIST_DIR/kwel-bridge"
