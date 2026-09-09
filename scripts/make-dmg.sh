#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
stage="$(mktemp -d "${TMPDIR:-/tmp}/fastaf-dmg.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
ditto 'dist/FastAF Flow.app' "$stage/FastAF Flow.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname 'FastAF Flow' -srcfolder "$stage" -ov -format UDZO 'dist/FastAF-Flow-1.0.0-macos-arm64.dmg'
python3 - <<'PY'
from pathlib import Path
import hashlib
artifacts = sorted(Path('dist').glob('FastAF-Flow-1.0.0-macos-arm64.*'))
Path('dist/SHA256SUMS').write_text(''.join(
    f'{hashlib.file_digest(p.open("rb"), "sha256").hexdigest()}  {p.name}\n' for p in artifacts
))
PY
