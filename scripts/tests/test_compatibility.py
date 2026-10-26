import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import compatibility as abi
import build

spec = importlib.util.spec_from_file_location("collect_release", SCRIPTS / "collect-release.py")
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class BinaryCompatibility(unittest.TestCase):
    def test_macho_reads_the_minimum_not_the_sdk_or_linker_version(self):
        legacy = "cmd LC_VERSION_MIN_MACOSX\n cmdsize 16\n version 10.12\n sdk 26.0"
        modern = "cmd LC_BUILD_VERSION\n cmdsize 32\n platform MACOS\n minos 11.0\n sdk 26.0\n ntools 1\n tool LD\n version 26000.1"
        self.assertEqual(abi.macho_minos(legacy), "10.12")
        self.assertEqual(abi.macho_minos(modern), "11.0")
        for invalid in ("sdk 26.0", modern.replace("MACOS", "IOS"), legacy + modern):
            with self.assertRaises(ValueError):
                abi.macho_minos(invalid)

    def test_macos_26_only_binary_is_rejected_for_each_slice(self):
        for incompatible in abi.MACOS_TARGETS:
            def inspect(*arguments):
                if "lipo" in str(arguments[0]):
                    return ""
                arch = arguments[arguments.index("-arch") + 1]
                minimum = "26.0" if arch == incompatible else abi.MACOS_TARGETS[arch]
                return f"cmd LC_BUILD_VERSION\n cmdsize 32\n platform MACOS\n minos {minimum}\n sdk 26.0"
            with patch.object(abi, "output", side_effect=inspect), self.assertRaises(ValueError):
                abi.verify_macos_binary(Path("fixture"))

    def test_glibc_239_and_supported_relr_are_accepted(self):
        self.assertEqual(abi.glibc_versions("Name: GLIBC_2.17\nName: GLIBC_2.39\nName: GCC_3.0"), (2, 39, 0))
        self.assertEqual(abi.glibc_versions("Name: GLIBC_ABI_DT_RELR\nName: GLIBC_2.2.5"), (2, 36, 0))

    def test_new_private_unknown_and_uninspected_glibc_requirements_fail(self):
        for text in ("Name: GLIBC_2.40", "Name: GLIBC_PRIVATE", "Name: GLIBC_ABI_UNKNOWN", "Name: GCC_3.0", "GLIBC_2.39"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                abi.glibc_versions(text)

    def test_elf_architecture_and_undeclared_dependencies_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "host"
            header = bytearray(64)
            header[:6] = b"\x7fELF\x02\x01"
            struct.pack_into("<H", header, 18, 62)
            binary.write_bytes(header)
            with self.assertRaises(ValueError):
                abi.verify_linux_binary(binary, "arm64")
            with patch.object(abi.shutil, "which", return_value="readelf"), patch.object(abi, "output", side_effect=["Name: GLIBC_2.39", "(NEEDED) Shared library: [libstdc++.so.6]"]), self.assertRaises(ValueError):
                abi.verify_linux_binary(binary, "amd64")

    def test_deb_inspects_host_client_and_both_executable_hooks(self):
        def unpack(*arguments, **unused):
            if "--extract" in arguments:
                root = Path(arguments[-1])
                for name in ("usr/lib/kwel-connect/kwel-native-host", "usr/bin/kwel-bridge"):
                    file = root / name
                    file.parent.mkdir(parents=True, exist_ok=True)
                    file.write_bytes(b"host fixture")
                    file.chmod(0o755)
                for name in build.HOSTS:
                    file = root / f"etc/opt/chrome/native-messaging-hosts/{name}.json"
                    file.parent.mkdir(parents=True, exist_ok=True)
                    file.write_text(json.dumps({"name": name, "path": "/usr/lib/kwel-connect/kwel-native-host", "allowed_origins": [f"chrome-extension://{build.EXTENSION}/"]}))
            if "--control" in arguments:
                root = Path(arguments[-1])
                root.mkdir()
                for name in ("preinst", "prerm"):
                    file = root / name
                    file.write_bytes(b"host fixture")
                    file.chmod(0o755)
        fields = {"Architecture": "amd64", "Version": build.VERSION, "Depends": "libc6 (>= 2.39), libgcc-s1"}
        # This is a synthetic Linux archive, including POSIX modes, on all test
        # hosts. Real archive modes are verified without mocks on Ubuntu CI.
        with patch.object(build.platform, "machine", return_value="x86_64"), patch.object(build, "run", side_effect=unpack), patch.object(build, "output_field", side_effect=lambda _, field: fields[field]), patch.object(build.stat, "S_IMODE", return_value=0o755), patch.object(build, "verify_linux_binary") as inspect:
            build.verify_linux_package(Path("fixture.deb"))
        self.assertEqual([call.args[0].name for call in inspect.call_args_list], ["kwel-native-host", "kwel-bridge", "preinst", "prerm"])


class ArtifactProvenance(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.source = Path(self.temporary.name) / "inputs"
        for artifact, (system, machine, files) in abi.ARTIFACTS.items():
            root = self.source / artifact
            root.mkdir(parents=True)
            for name in files:
                (root / name).write_bytes(("fixture " + name).encode())
            info = {"schema": 1, "version": collector.VERSION, "system": system, "machine": machine, "osId": "ubuntu" if system == "Linux" else "", "osVersion": {"Linux": "24.04", "Darwin": "26.0", "Windows": "10.0.26100"}[system], "sha256": {name: abi.digest(root / name) for name in files}}
            (root / abi.BUILDINFO).write_text(json.dumps(info))

    def test_all_five_release_files_keep_names_and_checksums(self):
        target = Path(self.temporary.name) / "release"
        collector.collect(self.source, target)
        self.assertEqual(len(list(target.iterdir())), 6)
        for line in (target / "SHA256SUMS").read_text().splitlines():
            digest, name = line.split("  ")
            self.assertEqual(digest, abi.digest(target / name))

    def test_ubuntu_26_release_producer_is_rejected_before_collection(self):
        path = self.source / "ubuntu-amd64" / abi.BUILDINFO
        info = json.loads(path.read_text())
        info["osVersion"] = "26.04"
        path.write_text(json.dumps(info))
        target = Path(self.temporary.name) / "release"
        with self.assertRaises(ValueError):
            collector.collect(self.source, target)
        self.assertFalse(target.exists())

    def test_missing_modified_or_wrong_architecture_artifact_fails_closed(self):
        root = self.source / "ubuntu-arm64"
        metadata = root / abi.BUILDINFO
        original = metadata.read_text()
        info = json.loads(original)
        info["machine"] = "x86_64"
        metadata.write_text(json.dumps(info))
        with self.assertRaises(ValueError):
            abi.verify_artifact(root, "ubuntu-arm64", collector.VERSION)
        metadata.write_text(original)
        package = root / "KwelConnect-ubuntu-arm64.deb"
        package.write_bytes(b"modified")
        with self.assertRaises(ValueError):
            abi.verify_artifact(root, "ubuntu-arm64", collector.VERSION)
        package.unlink()
        with self.assertRaises(ValueError):
            abi.verify_artifact(root, "ubuntu-arm64", collector.VERSION)


if __name__ == "__main__":
    unittest.main()
