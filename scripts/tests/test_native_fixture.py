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
        self.fixture.write_bytes(checker.FIXTURE.read_bytes())
        self.metadata = json.loads(checker.SOURCE.read_text())
        self.save_metadata()
        for name, value in (("FIXTURE", self.fixture), ("SOURCE", self.source)):
            patcher = patch.object(checker, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def save_metadata(self):
        self.source.write_text(json.dumps(self.metadata), encoding="utf-8")

    def test_canonical_corpus_is_verified_without_another_checkout(self):
        self.assertEqual(checker.verify(), self.metadata["fixtureSha256"])

    def test_changed_canonical_bytes_fail(self):
        self.fixture.write_bytes(self.fixture.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "fixture hash changed"):
            checker.verify()

    def test_invalid_digest_metadata_fails(self):
        for digest in (None, [], "", "0" * 63, "z" * 64):
            with self.subTest(digest=digest):
                self.metadata["fixtureSha256"] = digest
                self.save_metadata()
                with self.assertRaises(ValueError):
                    checker.verify()

    def test_wrong_canonical_owner_or_path_fails(self):
        for key, value in (("canonicalRepository", "other/project"),
                           ("canonicalPath", "../other.json"),
                           ("canonicalSource", "other.json")):
            with self.subTest(key=key):
                original = self.metadata[key]
                self.metadata[key] = value
                self.save_metadata()
                with self.assertRaises(ValueError):
                    checker.verify()
                self.metadata[key] = original

    def test_malformed_or_incomplete_metadata_fails(self):
        for metadata in ("{", "null", "[]", "{}"):
            with self.subTest(metadata=metadata):
                self.source.write_text(metadata)
                with self.assertRaises(ValueError):
                    checker.verify()

    def test_adoption_sources_are_history_not_files_to_read(self):
        self.metadata["commit"] = "a" * 40
        self.metadata["sources"] = [{"path": "missing/historical-file.ts", "sha256": "b" * 64}]
        self.save_metadata()
        self.assertEqual(checker.verify(), self.metadata["fixtureSha256"])


if __name__ == "__main__":
    unittest.main()
