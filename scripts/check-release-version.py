#!/usr/bin/env python3
"""Reject tags that do not match the production Workflow distribution."""
import json
import plistlib
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
config = json.loads((root / 'config/distribution.json').read_text(encoding='utf-8'))
info = plistlib.loads((root / 'workflow/info.plist').read_bytes())
version = config['version']
if len(sys.argv) != 2 or sys.argv[1] != 'v' + version:
    raise SystemExit('Release tag must be v' + version + '.')
if not re.fullmatch(r'[0-9]+\.[0-9]+(?:\.[0-9]+)?', version):
    raise SystemExit('Invalid distribution version.')
if info['version'] != version:
    raise SystemExit('Workflow and distribution versions differ.')
if config.get('buildChannel') != 'production' or config.get('kwelClientSupported') is not True:
    raise SystemExit('Release requires a production Kwel distribution with Alfred Integration.')
print('Verified release version ' + version)
