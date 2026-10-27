#!/usr/bin/env python3
import hashlib
from pathlib import Path
import shutil
import sys
import tomllib
from compatibility import ARTIFACTS, verify_artifact

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def collect(source, target):
    source, target = Path(source), Path(target)
    # Verify every producer and file before copying any release candidate.
    for artifact in ARTIFACTS:
        verify_artifact(source / artifact, artifact, VERSION)
    expected = {name: artifact for artifact, (_, _, names) in ARTIFACTS.items() for name in names}
    target.mkdir(parents=True, exist_ok=True)
    for name, artifact in expected.items():
        shutil.copyfile(source / artifact / name, target / name)
    (target / "SHA256SUMS").write_text("".join(f"{hashlib.sha256((target / name).read_bytes()).hexdigest()}  {name}\n" for name in sorted(expected)))
    assert set(path.name for path in target.iterdir()) == set(expected) | {"SHA256SUMS"}
    print("All five distribution files and SHA256SUMS assembled from verified release producers.")


if __name__ == "__main__":
    collect(sys.argv[1], sys.argv[2])
