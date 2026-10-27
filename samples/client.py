#!/usr/bin/env python3
"""Language-neutral JSON CLI integration; Python standard library only."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

def executable():
    if os.environ.get('KWEL_BRIDGE'):
        return os.environ['KWEL_BRIDGE']
    if shutil.which('kwel-bridge'):
        return shutil.which('kwel-bridge')
    if sys.platform=='darwin':
        return str(Path.home()/'Library/Application Support/Kwel/NativeBridge/bin/kwel-bridge')
    if sys.platform=='win32':
        return str(Path(os.environ['LOCALAPPDATA'])/'Kwel/NativeBridge/bin/kwel-bridge.exe')
    return '/usr/bin/kwel-bridge'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('operation',choices=['profiles','profile-probe','search','open'])
    parser.add_argument('--profile-id')
    parser.add_argument('--query',default='Kyoto')
    parser.add_argument('--ref')
    options=parser.parse_args()
    command=[executable(),options.operation,'--format','json']
    if options.operation!='profiles':
        if not options.profile_id:parser.error('--profile-id is required')
        command+=['--profile-id',options.profile_id]
    if options.operation=='search':command+=['--query',options.query,'--limit','20']
    if options.operation=='open':
        if not options.ref:parser.error('--ref is required')
        command+=['--ref',options.ref]
    # No shell interpolation, Rust dependency, or query/URL diagnostic logging.
    response=subprocess.run(command,capture_output=True,text=True,timeout=30)
    payload=json.loads(response.stdout)
    print(json.dumps(payload,ensure_ascii=False))
    return response.returncode
if __name__=='__main__':raise SystemExit(main())
