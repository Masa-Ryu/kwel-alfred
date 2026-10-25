#!/usr/bin/env python3
import hashlib
from pathlib import Path
import shutil
import sys
source=Path(sys.argv[1]);target=Path(sys.argv[2]);target.mkdir(parents=True,exist_ok=True)
expected={'KwelConnect-macos-universal.pkg':'macos-arm64','KwelConnect-windows-x64-setup.exe':'windows-x64','KwelConnect-ubuntu-amd64.deb':'ubuntu-amd64','KwelConnect-ubuntu-arm64.deb':'ubuntu-arm64','Kwel.alfredworkflow':'macos-arm64'}
for name,artifact in expected.items():
    path=source/artifact/name
    assert path.is_file() and not path.is_symlink() and path.stat().st_size>0,f'Missing artifact: {name}'
    shutil.copyfile(path,target/name)
(target/'SHA256SUMS').write_text(''.join(f'{hashlib.sha256((target/name).read_bytes()).hexdigest()}  {name}\n' for name in sorted(expected)))
assert set(path.name for path in target.iterdir())==set(expected)|{'SHA256SUMS'}
print('All five distribution files and SHA256SUMS assembled.')
