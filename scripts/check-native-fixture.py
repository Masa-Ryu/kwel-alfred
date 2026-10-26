#!/usr/bin/env python3
"""Check the canonical corpus and its immutable Kwel adoption provenance."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'crates/kwel-protocol/tests/fixtures/native-messaging-v1.json'
SOURCE = FIXTURE.with_suffix('.source.json')


def verify(kwel_root=None):
    source = json.loads(SOURCE.read_text(encoding='utf-8'))
    actual_hash = hashlib.sha256(FIXTURE.read_bytes()).hexdigest()
    if actual_hash != source['fixtureSha256']:
        raise ValueError('Shared Native v1 fixture hash changed; review and update its provenance.')
    if source['repository'] != 'Masa-Ryu/kwel' or not re.fullmatch(r'[0-9a-f]{40}', source['commit']):
        raise ValueError('Invalid pinned Kwel reference.')
    if kwel_root:
        kwel_root = Path(kwel_root).resolve()
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=kwel_root, text=True).strip()
        if head != source['commit']:
            raise ValueError('TypeScript checkout does not match the pinned Kwel commit.')
        for entry in source['sources']:
            for candidate in (PurePosixPath(entry['path']), PureWindowsPath(entry['path'])):
                if candidate.anchor or '..' in candidate.parts:
                    raise ValueError('Invalid reference path.')
            path = Path(entry['path'])
            if hashlib.sha256((kwel_root / path).read_bytes()).hexdigest() != entry['sha256']:
                raise ValueError(f'Pinned TypeScript reference differs: {path}')
        peer = kwel_root / 'tests/fixtures/local-connection/native-messaging-v1.json'
        if peer.read_bytes() != FIXTURE.read_bytes():
            raise ValueError('Rust and TypeScript are not testing the same JSON bytes.')
    return source['commit'], actual_hash


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--kwel-root')
    parser.add_argument('--print-kwel-ref', action='store_true')
    args = parser.parse_args()
    commit, digest = verify(args.kwel_root)
    if args.print_kwel_ref:
        print(commit)
    else:
        print(json.dumps({'result': 'passed', 'kwelCommit': commit, 'fixtureSha256': digest}))
