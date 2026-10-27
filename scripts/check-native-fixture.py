#!/usr/bin/env python3
"""Check Connect's canonical Native65 corpus identity and SHA-256."""
import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
CANONICAL_PATH = 'crates/kwel-protocol/tests/fixtures/native-messaging-v1.json'
FIXTURE = ROOT / CANONICAL_PATH
SOURCE = FIXTURE.with_suffix('.source.json')


def verify():
    source = json.loads(SOURCE.read_text(encoding='utf-8'))
    if not isinstance(source, dict) or (
        source.get('canonicalRepository') != 'Masa-Ryu/kwel-connect'
        or source.get('canonicalPath') != CANONICAL_PATH
        or source.get('canonicalSource') != CANONICAL_PATH
    ):
        raise ValueError('Invalid canonical corpus identity.')
    digest = source.get('fixtureSha256')
    if not isinstance(digest, str) or not re.fullmatch(r'[0-9a-f]{64}', digest):
        raise ValueError('Invalid canonical fixture digest.')
    actual_hash = hashlib.sha256(FIXTURE.read_bytes()).hexdigest()
    if actual_hash != digest:
        raise ValueError('Shared Native v1 fixture hash changed; review and update its provenance.')
    return actual_hash


if __name__ == '__main__':
    argparse.ArgumentParser(description=__doc__).parse_args()
    print(json.dumps({'result': 'passed', 'fixtureSha256': verify()}))
