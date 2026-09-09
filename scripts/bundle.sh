#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  echo 'Build the release on an Apple Silicon Mac.' >&2
  exit 1
fi
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build --release --locked
python3 scripts/licenses.py
mkdir -p dist
swift scripts/make-icon.swift dist/AppIcon.iconset
iconutil -c icns dist/AppIcon.iconset -o assets/AppIcon.icns
bundle='dist/FastAF Flow.app'
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources/worker" "$bundle/Contents/Resources/licenses"
cp target/release/fastaf-flow "$bundle/Contents/MacOS/fastaf-flow"
cp assets/Info.plist "$bundle/Contents/Info.plist"
cp assets/AppIcon.icns "$bundle/Contents/Resources/AppIcon.icns"
cp worker/engine.py worker/pyproject.toml worker/uv.lock "$bundle/Contents/Resources/worker/"
# uv is a standalone, redistributable installer. Python/MLX are installed only
# when the user clicks Install runtime; no developer venv or weights are bundled.
uv_path="${FASTAF_UV_BIN:-$(command -v uv)}"
cp "$uv_path" "$bundle/Contents/Resources/worker/uv"
chmod 755 "$bundle/Contents/Resources/worker/uv"
cp LICENSE "$bundle/Contents/Resources/LICENSE"
cp docs/licenses/* "$bundle/Contents/Resources/licenses/"
# Ad-hoc signing permits local execution, but is not Apple notarization.
codesign --force --sign - "$bundle/Contents/Resources/worker/uv"
codesign --force --sign - "$bundle/Contents/MacOS/fastaf-flow"
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
archive='dist/FastAF-Flow-1.0.0-macos-arm64.zip'
if [[ -e "$archive" ]]; then rm "$archive"; fi
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$archive"
printf 'Created %s\n' "$archive"
