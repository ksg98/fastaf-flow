#!/usr/bin/env python3
"""Maintainer-only: fetch missing crate notices from their pinned upstream commit."""
import json
from pathlib import Path
import subprocess
import urllib.request

meta = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"], text=True))
resolved = {n["id"] for n in meta["resolve"]["nodes"]}
out = Path("assets/license-notices")
out.mkdir(parents=True, exist_ok=True)
seen = set()
for p in meta["packages"]:
    if p["id"] not in resolved or p["name"] == "fastaf-flow":
        continue
    root = Path(p["manifest_path"]).parent
    patterns = ("LICENSE*", "LICENCE*", "COPYING*", "UNLICENSE*", "OFL*", "license*", "licenses/*")
    if any(f.is_file() for pattern in patterns for f in root.glob(pattern)):
        continue
    repo = (p.get("repository") or "").replace("http://github.com/", "https://github.com/").removesuffix(".git").rstrip("/")
    if not repo.startswith("https://github.com/"):
        print("REVIEW", p["name"], repo)
        continue
    vcs = json.loads((root / ".cargo_vcs_info.json").read_text())
    sha = vcs["git"]["sha1"]
    repo_path = "/".join(repo.split("/")[3:5])
    key = repo_path.replace("/", "_") + "_" + sha
    if key in seen or (out / (key + ".txt")).exists():
        continue
    seen.add(key)
    texts = []
    for filename in ("LICENSE", "LICENSE.txt", "LICENSE.md", "LICENSE-MIT", "LICENSE-APACHE", "LICENSE-MIT.txt", "LICENSE-APACHE.txt", "COPYING", "UNLICENSE"):
        url = f"https://raw.githubusercontent.com/{repo_path}/{sha}/{filename}"
        try:
            data = urllib.request.urlopen(url, timeout=15).read().decode()
            texts.append(f"Source: {url}\n\n{data}\n")
        except Exception:
            pass
    if texts:
        (out / (key + ".txt")).write_text("\n".join(texts))
        print(repo_path, sha[:8], "saved")
    else:
        print("REVIEW", p["name"], repo_path, sha)
