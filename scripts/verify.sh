#!/bin/bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "$0")/.." && pwd)"
export GOCACHE="${GOCACHE:-${TMPDIR:-/tmp}/kwel-alfred-go-cache}"
mkdir -p "$GOCACHE"
cd "$PROJECT_DIR"
go vet ./...
go test -race ./...
"$PROJECT_DIR/scripts/test-workflow-argv.sh"
"$PROJECT_DIR/scripts/package-workflow.sh" "${1:-$PROJECT_DIR/dist/Kwel.alfredworkflow}"
"$PROJECT_DIR/scripts/verify-package.sh" "${1:-$PROJECT_DIR/dist/Kwel.alfredworkflow}"
