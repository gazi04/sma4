#!/usr/bin/env bash
# Imports the ROM into a headless-only Ghidra project (separate from the GUI project),
# runs auto-analysis and exports decompiled C to tools/ghidra/export/.
# Usage: tools/ghidra/run_headless.sh /path/to/rom.gba
#   Re-export only (no re-import): tools/ghidra/run_headless.sh --export
#   Add Thumb pointer tables, then re-export:
#     tools/ghidra/run_headless.sh --ptr-tables 081317d8 6 [addr count ...]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
GHIDRA="${GHIDRA_HOME:-/opt/ghidra}"
PROJ_DIR="$HERE/project"
PROJ_NAME="sma4_headless"
OUT="$HERE/export"
mkdir -p "$PROJ_DIR" "$OUT"

if [[ "${1:-}" == "--export" ]]; then
    "$GHIDRA/support/analyzeHeadless" "$PROJ_DIR" "$PROJ_NAME" \
        -process -noanalysis -readOnly \
        -scriptPath "$HERE/scripts" \
        -postScript ExportDecomp.java "$OUT"
    exit 0
fi

if [[ "${1:-}" == "--ptr-tables" ]]; then
    shift
    "$GHIDRA/support/analyzeHeadless" "$PROJ_DIR" "$PROJ_NAME" \
        -process -noanalysis \
        -scriptPath "$HERE/scripts" \
        -postScript ThumbPtrTables.java "$@" \
        -postScript ExportDecomp.java "$OUT"
    exit 0
fi

ROM="${1:?usage: run_headless.sh /path/to/rom.gba}"
"$GHIDRA/support/analyzeHeadless" "$PROJ_DIR" "$PROJ_NAME" \
    -import "$ROM" -overwrite \
    -loader BinaryLoader -loader-baseAddr 0x08000000 \
    -processor "ARM:LE:32:v4t" \
    -scriptPath "$HERE/scripts" \
    -preScript GbaSetup.java \
    -postScript ExportDecomp.java "$OUT"
