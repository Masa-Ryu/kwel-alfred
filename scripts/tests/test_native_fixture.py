import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("native_fixture", SCRIPTS / "check-native-fixture.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class NativeFixtureProvenance(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.fixture = root / "canonical.json"
        self.source = root / "source.json"
        self.peer = root / "kwel"
        self.peer_fixture = self.peer / "tests/fixtures/local-connection/native-messaging-v1.json"
        self.peer_fixture.parent.mkdir(parents=True)
        corpus = checker.FIXTURE.read_bytes()
        self.fixture.write_bytes(corpus)
        self.peer_fixture.write_bytes(corpus)
        self.metadata = json.loads(checker.SOURCE.read_text())
        self.metadata["sources"] = [{
            "path": "tests/fixtures/local-connection/native-messaging-v1.json",
            "sha256": hashlib.sha256(corpus).hexdigest(),
        }]
        self.save_metadata()
        for name, value in (("FIXTURE", self.fixture), ("SOURCE", self.source)):
            patcher = patch.object(checker, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        head = patch.object(checker.subprocess, "check_output", return_value=self.metadata["commit"] + "\n")
        self.head = head.start()
        self.addCleanup(head.stop)

    def save_metadata(self):
        self.source.write_text(json.dumps(self.metadata), encoding="utf-8")

    def test_identical_corpus_and_pinned_checkout_pass(self):
        self.assertEqual(checker.verify(self.peer), (self.metadata["commit"], self.metadata["fixtureSha256"]))

    def test_changed_canonical_bytes_fail_before_checkout(self):
        self.fixture.write_bytes(self.fixture.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "fixture hash changed"):
            checker.verify(self.peer)
        self.head.assert_not_called()

    def test_wrong_checkout_head_fails(self):
        self.head.return_value = "0" * 40 + "\n"
        with self.assertRaisesRegex(ValueError, "pinned Kwel commit"):
            checker.verify(self.peer)

    def test_modified_reference_source_fails(self):
        self.peer_fixture.write_bytes(self.peer_fixture.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "reference differs"):
            checker.verify(self.peer)

    def test_matching_reference_metadata_cannot_hide_different_corpus(self):
        self.peer_fixture.write_bytes(self.peer_fixture.read_bytes() + b"\n")
        self.metadata["sources"][0]["sha256"] = hashlib.sha256(self.peer_fixture.read_bytes()).hexdigest()
        self.save_metadata()
        with self.assertRaisesRegex(ValueError, "same JSON bytes"):
            checker.verify(self.peer)

    def test_reference_paths_cannot_escape_checkout(self):
        for path in ("../outside.json", "/outside.json"):
            with self.subTest(path=path):
                self.metadata["sources"][0]["path"] = path
                self.save_metadata()
                with self.assertRaisesRegex(ValueError, "Invalid reference path"):
                    checker.verify(self.peer)


if __name__ == "__main__":
    unittest.main()
