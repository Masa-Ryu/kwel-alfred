#!/usr/bin/env python3
"""Read-only guards for an explicitly dispatched release of a main snapshot."""
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]


def validate(*, event, ref, sha, tag, repository, root=ROOT, run=subprocess.run):
    if event != 'workflow_dispatch':
        raise ValueError('Release requires workflow_dispatch.')
    if ref != 'refs/heads/main':
        raise ValueError('Release must be dispatched from main.')
    if not re.fullmatch(r'(?:[a-f0-9]{40}|[a-f0-9]{64})', sha or ''):
        raise ValueError('Invalid execution SHA.')
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*', repository or ''):
        raise ValueError('Invalid repository context.')

    def checked(command, message):
        result = run(command, cwd=root, capture_output=True, text=True, check=False)
        if result.returncode:
            raise ValueError(message)
        return result.stdout.strip()

    checked([sys.executable, str(root / 'scripts/check-release-version.py'), tag or ''],
            'Tag and production distribution version must agree.')
    head = checked(['git', 'rev-parse', '--verify', 'HEAD'], 'Cannot resolve checkout.')
    if head != sha:
        raise ValueError('Checkout and execution SHA differ.')
    target = checked(['git', 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}'],
                     'Tag must exist and resolve to a commit.')
    if target != sha:
        raise ValueError('Tag and execution SHA differ.')
    checked(['git', 'merge-base', '--is-ancestor', sha, 'refs/remotes/origin/main'],
            'Execution commit must be contained in main.')
    # Only a real HTTP 404 means absence. Authorization/network/API failures
    # must not be mistaken for permission to create or overwrite a Release.
    result = run(['gh', 'api', '--method', 'GET', '--include',
                  f'repos/{repository}/releases/tags/{quote(tag, safe="")}'],
                 cwd=root, capture_output=True, text=True, check=False)
    if result.returncode == 0:
        raise ValueError('A Release already exists; refusing to overwrite it.')
    if not re.match(r'HTTP/\S+\s+404(?:\s|$)', result.stdout):
        raise ValueError('Could not confirm Release absence through the GitHub API.')
    return sha


if __name__ == '__main__':
    try:
        validate(event=os.environ.get('RELEASE_EVENT'), ref=os.environ.get('RELEASE_REF'),
                 sha=os.environ.get('RELEASE_SHA'), tag=os.environ.get('RELEASE_TAG'),
                 repository=os.environ.get('GH_REPO'))
    except (ValueError, OSError) as error:
        print(f'Release preflight failed: {error}', file=sys.stderr)
        sys.exit(1)
    print('Manual main snapshot, existing tag, versions and Release absence verified.')
