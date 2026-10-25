#!/bin/bash
set -euo pipefail

# Repository policy permits only owner-maintained Actions. Use official,
# checksum-pinned tools instead of external setup or artifact Actions.
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  printf '%s\n' 'CI tools require the Linux x86_64 n100 runner.' >&2
  exit 1
fi
TOOL_DIR="$(mktemp -d "$RUNNER_TEMP/kwel-tools.XXXXXX")"
curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
  https://go.dev/dl/go1.27.3.linux-amd64.tar.gz --output "$TOOL_DIR/go.tar.gz"
printf '%s  %s\n' 65efbc80fdc1fa2cd2631cad5b481777a68e8725f34e89f50fc6ac714e379b40 "$TOOL_DIR/go.tar.gz" | sha256sum --check
tar -xzf "$TOOL_DIR/go.tar.gz" -C "$TOOL_DIR"
curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
  https://github.com/linebender/resvg/releases/download/v0.45.1/resvg-linux-x86_64.tar.gz \
  --output "$TOOL_DIR/resvg.tar.gz"
printf '%s  %s\n' 3c1d52850a78bc4ed0e9d05605b53ad21bb5313184f14bf72515d29cb98cda10 "$TOOL_DIR/resvg.tar.gz" | sha256sum --check
tar -xzf "$TOOL_DIR/resvg.tar.gz" -C "$TOOL_DIR" resvg
"$TOOL_DIR/go/bin/go" version
"$TOOL_DIR/resvg" --version
printf '%s\n' "$TOOL_DIR/go/bin" "$TOOL_DIR" >> "$GITHUB_PATH"
printf 'GOCACHE=%s\nGOMODCACHE=%s\n' "$TOOL_DIR/go-cache" "$TOOL_DIR/go-modcache" >> "$GITHUB_ENV"
