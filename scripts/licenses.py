#!/usr/bin/env python3
"""Collect license texts for Rust dependencies shipped in the native binary."""
import json
from pathlib import Path
import subprocess

meta = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--format-version", "1",
    "--filter-platform", "aarch64-apple-darwin"], text=True))
tree = subprocess.check_output(["cargo", "tree", "--locked", "--target", "aarch64-apple-darwin", "--edges", "normal,build", "--prefix", "none", "--format", "{p}"], text=True)
active = {tuple(line.split()[:2]) for line in tree.splitlines()}
resolved = {p["id"] for p in meta["packages"] if (p["name"], "v" + p["version"]) in active}
parts = ["FastAF Flow: third-party Rust dependency notices\nGenerated from Cargo.lock.\n"]
missing = []
for package in sorted(meta["packages"], key=lambda p: p["name"]):
    if package["id"] not in resolved or package["name"] == "fastaf-flow":
        continue
    root = Path(package["manifest_path"]).parent
    parts.append(f'\n{"=" * 72}\n{package["name"]} {package["version"]}\nLicense: {package.get("license")}\nSource: {package.get("repository") or package.get("homepage") or "https://crates.io/crates/" + package["name"]}\n')
    candidates = set()
    if package.get("license_file"):
        candidates.add(root / package["license_file"])
    for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "UNLICENSE*", "OFL*", "license*", "licenses/*"):
        candidates.update(root.glob(pattern))
    files = sorted(p for p in candidates if p.is_file())
    if not files:
        vcs_file = root / ".cargo_vcs_info.json"
        if vcs_file.exists():
            sha = json.loads(vcs_file.read_text())["git"]["sha1"]
            files = sorted(Path("assets/license-notices").glob("*_" + sha + ".txt"))
        if not files:
            missing.append(package["name"])
    for path in files:
        parts.append(f"\n--- {path.name} ---\n{path.read_text(errors='replace')}\n")
Path("docs/licenses/Rust-THIRD-PARTY.txt").write_text("\n".join(parts))
print(f"Collected notices for {len(resolved) - 1} crates. No packaged license file: {', '.join(missing) or 'none'}")
