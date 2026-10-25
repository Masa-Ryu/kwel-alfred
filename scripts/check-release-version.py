#!/usr/bin/env python3
import json
from pathlib import Path
import plistlib
import re
import sys
import tomllib
root=Path(__file__).resolve().parents[1]
version=tomllib.loads((root/'Cargo.toml').read_text())['workspace']['package']['version']
assert re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?',version),'SemVer is required'
assert len(sys.argv)==2 and sys.argv[1]=='v'+version,'Tag and distribution version differ'
config=json.loads((root/'config/distribution.json').read_text())
assert config['version']==version and config['buildChannel']=='production' and config['kwelClientSupported'] is True
assert config['chromeExtensionId']=='mhablbmgafbdofgaffhdnnknjhafmlin'
assert plistlib.loads((root/'workflow/alfred/info.plist').read_bytes())['version']==version
print('Tag, Cargo, production configuration and Alfred version agree.')
