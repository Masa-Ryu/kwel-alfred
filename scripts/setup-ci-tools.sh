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

# Extract missing apt tools into this job's temporary directory. apt verifies
# repository signatures and package hashes; no system package or Docker access
# is needed. The runner's compiler and Python remain required prerequisites.
packages=()
if command -v llvm-lipo >/dev/null; then
  LIPO_PATH="$(command -v llvm-lipo)"
elif command -v llvm-lipo-18 >/dev/null; then
  LIPO_PATH="$(command -v llvm-lipo-18)"
else
  packages+=(llvm-18 libllvm18)
  LIPO_PATH="$TOOL_DIR/llvm-lipo"
fi
if [[ "${KWEL_RELEASE:-0}" == 1 ]] && ! command -v gh >/dev/null; then
  packages+=(gh)
fi
if (( ${#packages[@]} )); then
  mkdir -p "$TOOL_DIR/apt-lists/partial" "$TOOL_DIR/apt-cache/archives/partial" "$TOOL_DIR/debs"
  apt_options=(-o "Dir::State::lists=$TOOL_DIR/apt-lists" -o "Dir::Cache=$TOOL_DIR/apt-cache")
  apt-get "${apt_options[@]}" update
  (cd "$TOOL_DIR/debs" && apt-get "${apt_options[@]}" download "${packages[@]}")
  for package in "$TOOL_DIR/debs/"*.deb; do
    dpkg-deb --extract "$package" "$TOOL_DIR/apt-tools"
  done
fi
if [[ "$LIPO_PATH" == "$TOOL_DIR/llvm-lipo" ]]; then
  cat > "$LIPO_PATH" <<'LIPO'
#!/bin/sh
TOOL_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
LD_LIBRARY_PATH="$TOOL_DIR/apt-tools/usr/lib/x86_64-linux-gnu:$TOOL_DIR/apt-tools/usr/lib/llvm-18/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  exec "$TOOL_DIR/apt-tools/usr/bin/llvm-lipo-18" "$@"
LIPO
  chmod 755 "$LIPO_PATH"
fi
"$LIPO_PATH" -version
printf 'LLVM_LIPO=%s\n' "$LIPO_PATH" >> "$GITHUB_ENV"
if [[ -d "$TOOL_DIR/apt-tools/usr/bin" ]]; then
  printf '%s\n' "$TOOL_DIR/apt-tools/usr/bin" >> "$GITHUB_PATH"
fi
