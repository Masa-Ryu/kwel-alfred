import importlib.util
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def block(source, key, indent=0):
    lines = source.splitlines()
    start = next(index for index, line in enumerate(lines) if line.startswith(' ' * indent + key + ':'))
    end = start + 1
    while end < len(lines):
        line = lines[end]
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        end += 1
    return '\n'.join(lines[start:end])


class ReleasePolicy(unittest.TestCase):
    def setUp(self):
        self.source = (ROOT / '.github/workflows/release.yml').read_text()

    def test_only_explicit_dispatch_with_required_tag_starts_release(self):
        trigger = block(self.source, 'on')
        self.assertEqual(re.findall(r'^  ([\w-]+):', trigger, re.M), ['workflow_dispatch'])
        tag = block(trigger, 'tag', 6)
        self.assertRegex(tag, r'type: string')
        self.assertRegex(tag, r'required: true')

    def test_only_validated_snapshot_can_reach_publication(self):
        jobs = block(self.source, 'jobs')
        preflight, validate, publish = (block(jobs, name, 2) for name in ('preflight', 'validate', 'publish'))
        self.assertIn('scripts/release-preflight.py', preflight)
        self.assertIn('needs: preflight', validate)
        self.assertIn('uses: ./.github/workflows/test.yml', validate)
        self.assertIn('release-artifacts: true', validate)
        self.assertIn('needs: [preflight, validate]', publish)
        self.assertNotRegex(publish, r'(?m)^    (?:if|continue-on-error):')
        self.assertNotRegex(validate, r'(?m)^    (?:if|continue-on-error):')
        self.assertEqual(self.source.count('ref: ${{ github.sha }}'), 2)
        self.assertEqual(self.source.count('fetch-depth: 0'), 2)
        self.assertIn('scripts/release-preflight.py', publish)
        self.assertEqual(self.source.count('contents: write'), 1)
        self.assertIn('contents: write', publish)
        self.assertEqual(block(self.source, 'permissions'), 'permissions:\n  contents: read')
        for action in re.findall(r'uses: ([^\s]+)', self.source):
            if not action.startswith('./'):
                self.assertRegex(action, r'@[a-f0-9]{40}$')

    def test_publication_keeps_checksums_draft_and_existing_release_guards(self):
        publish = block(block(self.source, 'jobs'), 'publish', 2)
        self.assertIn('python3 scripts/collect-release.py release-inputs release', publish)
        self.assertIn('sha256sum --check SHA256SUMS', publish)
        self.assertIn('gh release view "$RELEASE_TAG"', publish)
        self.assertIn('--verify-tag --draft', publish)
        self.assertLess(publish.index('sha256sum --check'), publish.index('gh release create'))
        self.assertLess(publish.index('gh release create'), publish.index('gh release edit'))
        self.assertIn('gh release edit "$RELEASE_TAG" --draft=false', publish)
        self.assertIn('RELEASE_TAG: ${{ inputs.tag }}', self.source)
        self.assertNotIn('${{ github.ref_name }}', self.source)
        self.assertNotRegex(self.source, r'git (?:tag|push)\b')


class TestWorkflowPolicy(unittest.TestCase):
    def test_automatic_and_reusable_triggers_are_preserved(self):
        source = (ROOT / '.github/workflows/test.yml').read_text()
        triggers = block(source, 'on')
        self.assertEqual(re.findall(r'^  ([\w-]+):', triggers, re.M),
                         ['pull_request', 'push', 'workflow_dispatch', 'workflow_call'])
        self.assertIn("branches: ['**']", block(triggers, 'push', 2))

    def test_push_pr_and_release_validation_cannot_cancel_each_other(self):
        source = (ROOT / '.github/workflows/test.yml').read_text()
        concurrency = block(source, 'concurrency')
        # Distinct event streams and caller workflows must have distinct keys.
        # A newer run within the same stream may still cancel an obsolete one.
        self.assertIn('${{ github.event_name }}', concurrency)
        self.assertIn('${{ github.workflow }}', concurrency)
        self.assertIn('github.event.pull_request.head.repo.full_name || github.repository', concurrency)
        self.assertIn('github.event.pull_request.head.ref || github.ref_name', concurrency)
        self.assertIn('cancel-in-progress: true', concurrency)


class ReleasePreflight(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location('release_preflight', ROOT / 'scripts/release-preflight.py')
        cls.preflight = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.preflight)

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name in ('Cargo.toml', 'config/distribution.json', 'workflow/alfred/info.plist', 'scripts/check-release-version.py'):
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, target)
        self.git('init', '--quiet', '--initial-branch=main')
        self.git('config', 'core.autocrlf', 'false')
        self.git('add', '.')
        self.git('commit', '--quiet', '-m', 'initial fixture')
        self.old = self.git('rev-parse', 'HEAD')
        (self.root / 'main.txt').write_text('main snapshot\n')
        self.git('add', '.')
        self.git('commit', '--quiet', '-m', 'main fixture')
        self.sha = self.git('rev-parse', 'HEAD')
        self.git('update-ref', 'refs/remotes/origin/main', self.sha)
        self.version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
        self.tag = 'v' + self.version
        self.git('tag', self.tag, self.sha)
        self.context = dict(event='workflow_dispatch', ref='refs/heads/main', sha=self.sha, tag=self.tag, repository='Masa-Ryu/kwel-connect')
        self.api = subprocess.CompletedProcess([], 1, 'HTTP/2.0 404 Not Found\n\n{}', 'not found')
        self.requests = []

    def git(self, *args):
        return subprocess.check_output(['git', '-c', 'user.name=Release Fixture', '-c', 'user.email=fixture@example.test', '-c', 'commit.gpgsign=false', '-c', 'tag.gpgsign=false', *args], cwd=self.root, text=True).strip()

    def runner(self, command, **kwargs):
        if command[0] == 'gh':
            self.requests.append(command)
            self.assertEqual(command[1:5], ['api', '--method', 'GET', '--include'])
            return self.api
        return subprocess.run(command, **kwargs)

    def validate(self, **changes):
        return self.preflight.validate(**(self.context | changes), root=self.root, run=self.runner)

    def test_manual_main_snapshot_and_existing_lightweight_tag_pass(self):
        self.assertEqual(self.validate(), self.sha)
        self.assertEqual(len(self.requests), 1)

    def test_annotated_tag_is_resolved_to_commit(self):
        self.git('tag', '-d', self.tag)
        self.git('tag', '-a', self.tag, self.sha, '-m', 'reviewed fixture')
        self.assertEqual(self.validate(), self.sha)

    def test_tag_and_main_push_events_cannot_release(self):
        for ref in ('refs/tags/v1.0.0', 'refs/heads/main'):
            with self.subTest(ref=ref), self.assertRaisesRegex(ValueError, 'workflow_dispatch'):
                self.validate(event='push', ref=ref)
        self.assertFalse(self.requests)

    def test_dispatch_from_branch_or_tag_is_rejected(self):
        for ref in ('refs/heads/review', 'refs/tags/v1.0.0'):
            with self.subTest(ref=ref), self.assertRaisesRegex(ValueError, 'main'):
                self.validate(ref=ref)
        self.assertFalse(self.requests)

    def test_nonexistent_tag_is_rejected_before_api(self):
        self.git('tag', '-d', self.tag)
        with self.assertRaisesRegex(ValueError, 'Tag must exist'):
            self.validate()
        self.assertFalse(self.requests)

    def test_tag_pointing_to_another_main_commit_is_rejected(self):
        self.git('tag', '-f', self.tag, self.old)
        with self.assertRaisesRegex(ValueError, 'Tag and execution SHA'):
            self.validate()
        self.assertFalse(self.requests)

    def test_noncommit_tag_is_rejected(self):
        self.git('tag', '-f', self.tag, self.git('rev-parse', 'HEAD:main.txt'))
        with self.assertRaisesRegex(ValueError, 'Tag must exist'):
            self.validate()

    def test_changed_checkout_and_main_history_are_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Checkout'):
            self.validate(sha=self.old)
        self.git('update-ref', 'refs/remotes/origin/main', self.old)
        with self.assertRaisesRegex(ValueError, 'contained in main'):
            self.validate()

    def test_main_moving_after_dispatch_does_not_change_selected_snapshot(self):
        self.git('checkout', '--quiet', '--detach', self.old)
        self.git('tag', '-f', self.tag, self.old)
        self.assertEqual(self.validate(sha=self.old), self.old)

    def test_invalid_or_mismatching_version_is_rejected(self):
        for tag in ('v' + self.version + '-different', self.version, self.tag + '/../main', 'v01.0.0', self.tag + '\nother'):
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, 'version'):
                self.validate(tag=tag)
        self.assertFalse(self.requests)

    def set_versions(self, version):
        import json
        import plistlib
        manifest = (ROOT / 'Cargo.toml').read_text().replace('version = "' + self.version + '"', 'version = "' + version + '"', 1)
        (self.root / 'Cargo.toml').write_text(manifest)
        config = json.loads((ROOT / 'config/distribution.json').read_text())
        config['version'] = version
        (self.root / 'config/distribution.json').write_text(json.dumps(config))
        workflow = plistlib.loads((ROOT / 'workflow/alfred/info.plist').read_bytes())
        workflow['version'] = version
        (self.root / 'workflow/alfred/info.plist').write_bytes(plistlib.dumps(workflow))

    def test_semver_identifiers_reject_leading_zeroes_and_empty_components(self):
        for version in ('01.0.0', '1.0', '1.0.0-01', '1.0.0-.', '1.0.0-alpha..1', '1.0.0+'):
            self.set_versions(version)
            with self.subTest(version=version), self.assertRaisesRegex(ValueError, 'version'):
                self.validate(tag='v' + version)
        self.assertFalse(self.requests)

    def test_valid_semver_prerelease_and_build_identifiers_pass(self):
        version = '1.0.0-rc.1+review.01'
        self.set_versions(version)
        self.git('add', '.')
        self.git('commit', '--quiet', '-m', 'version fixture')
        sha = self.git('rev-parse', 'HEAD')
        self.git('update-ref', 'refs/remotes/origin/main', sha)
        self.git('tag', 'v' + version, sha)
        self.assertEqual(self.validate(tag='v' + version, sha=sha), sha)

    def test_recheck_rejects_tag_moved_after_initial_preflight(self):
        self.assertEqual(self.validate(), self.sha)
        self.git('tag', '-f', self.tag, self.old)
        with self.assertRaisesRegex(ValueError, 'Tag and execution SHA'):
            self.validate()

    def test_distribution_and_alfred_version_checks_remain_required(self):
        import json
        import plistlib
        config = self.root / 'config/distribution.json'
        value = json.loads(config.read_text())
        value['version'] = self.version + '-different'
        config.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'version'):
            self.validate()
        shutil.copyfile(ROOT / 'config/distribution.json', config)
        workflow = self.root / 'workflow/alfred/info.plist'
        value = plistlib.loads(workflow.read_bytes())
        value['version'] = self.version + '-different'
        workflow.write_bytes(plistlib.dumps(value))
        with self.assertRaisesRegex(ValueError, 'version'):
            self.validate()

    def test_existing_release_and_draft_are_rejected(self):
        for draft in ('true', 'false'):
            self.api = subprocess.CompletedProcess([], 0, 'HTTP/2.0 200 OK\n\n{"draft":' + draft + '}', '')
            with self.subTest(draft=draft), self.assertRaisesRegex(ValueError, 'already exists'):
                self.validate()

    def test_api_failures_are_not_mistaken_for_missing_release(self):
        for status in ('HTTP/2.0 403 Forbidden', 'HTTP/2.0 500 Internal Server Error', ''):
            self.api = subprocess.CompletedProcess([], 1, status, 'network or authorization error')
            with self.subTest(status=status), self.assertRaisesRegex(ValueError, 'confirm Release absence'):
                self.validate()

    def test_invalid_context_is_rejected_without_external_calls(self):
        for changes in ({'sha': '--bad'}, {'repository': '../outside'}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.validate(**changes)
        self.assertFalse(self.requests)
