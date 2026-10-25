#!/usr/bin/env python3
"""Build/package the current OS. No Go, private preferences, or user IDs are read."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
EXTENSION = json.loads((ROOT / "config/distribution.json").read_text())["chromeExtensionId"]
HOSTS = ("com.kwel.bridge", "com.kwel.alfred")
BINS = ("kwel-native-host", "kwel-bridge", "kwel-alfred")


def run(*arguments, **kwargs):
    return subprocess.run([str(value) for value in arguments], check=True, **kwargs)


def build():
    destination = DIST / "bin"
    destination.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment["RUSTFLAGS"] = environment.get("RUSTFLAGS", "") + f" --remap-path-prefix={Path.home()}=/build --remap-path-prefix={ROOT}=."
    if sys.platform == "darwin":
        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
            run("cargo", "build", "--workspace", "--release", "--locked", "--target", target, cwd=ROOT, env=environment)
        for name in BINS:
            run("lipo", "-create", *(ROOT / "target" / target / "release" / name for target in ("aarch64-apple-darwin", "x86_64-apple-darwin")), "-output", destination / name)
            for architecture in ("arm64", "x86_64"):
                run("/usr/bin/lipo", destination / name, "-verify_arch", architecture)
            (destination / name).chmod(0o700)
    else:
        run("cargo", "build", "--workspace", "--release", "--locked", cwd=ROOT, env=environment)
        for name in BINS:
            filename = name + (".exe" if sys.platform == "win32" else "")
            shutil.copyfile(ROOT / "target/release" / filename, destination / filename)
            if sys.platform != "win32":
                (destination / filename).chmod(0o755)


def icon(destination):
    renderer = os.environ.get("RESVG", "resvg")
    version = run(renderer, "--version", capture_output=True, text=True).stdout.strip()
    if version != "0.45.1":
        raise ValueError("resvg 0.45.1 is required")
    run(renderer, "--skip-system-fonts", "--width", "512", "--height", "512", ROOT / "workflow/alfred/assets/kwel-light.svg", destination)
    data = destination.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("Invalid PNG")
    clean = bytearray(data[:8])
    offset = 8
    while offset < len(data):
        length = int.from_bytes(data[offset:offset + 4], "big")
        end = offset + length + 12
        if end > len(data):
            raise ValueError("Truncated PNG")
        if data[offset + 4:offset + 8] not in {b"eXIf", b"tEXt", b"zTXt", b"iTXt", b"tIME"}:
            clean.extend(data[offset:end])
        offset = end
    destination.write_bytes(clean)


def workflow():
    if sys.platform != "darwin":
        raise ValueError("The official Alfred package is built on macOS")
    with tempfile.TemporaryDirectory(prefix="kwel-workflow-") as temporary:
        stage = Path(temporary)
        shutil.copyfile(ROOT / "workflow/alfred/info.plist", stage / "info.plist")
        info = plistlib.loads((stage / "info.plist").read_bytes())
        assert info["version"] == VERSION
        icon(stage / "icon.png")
        (stage / "scripts").mkdir()
        for name in ("search.sh", "open-result.sh"):
            shutil.copyfile(ROOT / "workflow/alfred/scripts" / name, stage / "scripts" / name)
        for name in BINS:
            shutil.copyfile(DIST / "bin" / name, stage / name)
        for name in ("README.md", "LICENSE"):
            shutil.copyfile(ROOT / name, stage / name)
        with zipfile.ZipFile(DIST / "Kwel.alfredworkflow", "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for path in sorted(stage.rglob("*")):
                if not path.is_file():
                    continue
                name = path.relative_to(stage).as_posix()
                executable = name in BINS or name.startswith("scripts/")
                entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                entry.create_system = 3
                entry.external_attr = (stat.S_IFREG | (0o755 if executable else 0o644)) << 16
                entry.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(entry, path.read_bytes(), compresslevel=9)


def macos():
    with tempfile.TemporaryDirectory(prefix="kwel-pkg-") as temporary:
        stage = Path(temporary)
        payload = stage / "payload"
        binaries = payload / "Library/Application Support/Kwel/NativeBridge/installer-payload"
        binaries.mkdir(parents=True)
        for name in BINS[:2]:
            shutil.copyfile(DIST / "bin" / name, binaries / name)
        application = payload / "Library/Application Support/Kwel/NativeBridge/Kwel Connect Uninstaller.app/Contents"
        (application / "MacOS").mkdir(parents=True)
        shutil.copyfile(ROOT / "packaging/macos/uninstall", application / "MacOS/uninstall")
        (application / "Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "com.kwel.connect.uninstaller", "CFBundleName": "Kwel Connect Uninstaller", "CFBundleExecutable": "uninstall", "CFBundlePackageType": "APPL", "CFBundleVersion": VERSION}))
        for path in payload.rglob("*"):
            path.chmod(0o700 if path.is_dir() or path.name in BINS or path.name == "uninstall" else 0o600)
        scripts = stage / "scripts"
        scripts.mkdir()
        shutil.copyfile(ROOT / "packaging/macos/postinstall", scripts / "postinstall")
        (scripts / "postinstall").chmod(0o755)
        component = stage / "component.pkg"
        run("pkgbuild", "--root", payload, "--scripts", scripts, "--identifier", "com.kwel.connect", "--version", VERSION, "--ownership", "recommended", "--install-location", "/", component)
        distribution = (ROOT / "packaging/macos/distribution.xml").read_text().replace("@VERSION@", VERSION)
        (stage / "distribution.xml").write_text(distribution)
        run("productbuild", "--distribution", stage / "distribution.xml", "--package-path", stage, DIST / "KwelConnect-macos-universal.pkg")


def windows():
    compiler = shutil.which("makensis") or r"C:\Program Files (x86)\NSIS\makensis.exe"
    run(compiler, f"/DVERSION={VERSION}", f"/DPAYLOAD={DIST / 'bin'}", f"/DOUTPUT={DIST / 'KwelConnect-windows-x64-setup.exe'}", ROOT / "packaging/windows/installer.nsi")


def linux():
    architecture = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
    with tempfile.TemporaryDirectory(prefix="kwel-deb-") as temporary:
        stage = Path(temporary)
        control = stage / "DEBIAN"
        control.mkdir()
        (control / "control").write_text(f"Package: kwel-connect\nVersion: {VERSION}\nArchitecture: {architecture}\nMaintainer: Kwel <noreply@kwel.ryu.zone>\nDepends: libc6 (>= 2.39)\nDescription: Kwel official local integration Native Host and JSON client\nHomepage: https://github.com/Masa-Ryu/kwel-connect\n")
        host = stage / "usr/lib/kwel-connect/kwel-native-host"
        client = stage / "usr/bin/kwel-bridge"
        host.parent.mkdir(parents=True)
        client.parent.mkdir(parents=True)
        shutil.copyfile(DIST / "bin/kwel-native-host", host)
        shutil.copyfile(DIST / "bin/kwel-bridge", client)
        # Rust executable hooks perform strict JSON preflight and in-use checks.
        for name in ("preinst", "prerm"):
            shutil.copyfile(host, control / name)
            (control / name).chmod(0o755)
        release = {"product": "kwel-connect", "version": VERSION, "protocol": 1, "binaries": {name: hashlib.sha256((DIST / "bin" / name).read_bytes()).hexdigest() for name in BINS[:2]}}
        (host.parent / "release.json").write_text(json.dumps(release, indent=2) + "\n")
        manifests = stage / "etc/opt/chrome/native-messaging-hosts"
        manifests.mkdir(parents=True)
        for name in HOSTS:
            (manifests / f"{name}.json").write_text(json.dumps({"name": name, "description": "Kwel Local Integration", "path": "/usr/lib/kwel-connect/kwel-native-host", "type": "stdio", "allowed_origins": [f"chrome-extension://{EXTENSION}/"]}, indent=2) + "\n")
        for path in stage.rglob("*"):
            if path.is_file():
                path.chmod(0o755 if path.name in (*BINS, "preinst", "prerm") else 0o644)
            else:
                path.chmod(0o755)
        run("dpkg-deb", "--root-owner-group", "--build", stage, DIST / f"KwelConnect-ubuntu-{architecture}.deb")


def verify_workflow(path):
    expected = {"info.plist", "icon.png", "README.md", "LICENSE", *BINS, "scripts/search.sh", "scripts/open-result.sh"}
    with zipfile.ZipFile(path) as archive:
        assert len(archive.namelist()) == len(set(archive.namelist()))
        assert set(archive.namelist()) == expected
        for entry in archive.infolist():
            executable = entry.filename in BINS or entry.filename.startswith("scripts/")
            mode = entry.external_attr >> 16
            assert entry.create_system == 3 and stat.S_ISREG(mode)
            assert stat.S_IMODE(mode) == (0o755 if executable else 0o644)
            archive.read(entry)
        info = plistlib.loads(archive.read("info.plist"))
        assert info["bundleid"] == "com.kwel.alfred.search" and info["version"] == VERSION
        field, = info["userconfigurationconfig"]
        assert field["variable"] == "KWEL_PROFILE_ID" and field["config"]["required"] and field["config"]["default"] == ""
        assert not info.get("variables", {}).get("KWEL_PROFILE_ID")
        assert info["webaddress"] == "https://kwel.ryu.zone"
        assert "github.com/Masa-Ryu/kwel-connect/releases" in info["readme"]
        objects = info["objects"]
        assert len(objects) == 4
        search = next(value for value in objects if value["type"] == "alfred.workflow.input.scriptfilter")
        fallback = next(value for value in objects if value["type"] == "alfred.workflow.trigger.fallback")
        assert fallback["config"]["text"] == "Search Kwel"
        assert not search["config"]["keyword"] and not search["config"]["alfredfiltersresults"]
        assert "inboundconfig" not in search["config"] and search["config"]["scriptargtype"] == 1
        assert search["config"]["scriptfile"] == "./scripts/search.sh"
        assert len(info["connections"]) == 3
        assert b'./kwel-alfred search --profile-id "$profile_id" --query "$query" --limit 20 --format alfred' in archive.read("scripts/search.sh")
        assert b'./kwel-alfred open --profile-id "$profile_id" --ref "$open_ref"' in archive.read("scripts/open-result.sh")
        icon_bytes = archive.read("icon.png")
        assert icon_bytes[:8] == b"\x89PNG\r\n\x1a\n" and struct.unpack(">II", icon_bytes[16:24]) == (512, 512)
        with tempfile.TemporaryDirectory(prefix="kwel-universal-") as temporary:
            for name in BINS:
                binary = Path(temporary) / name
                data = archive.read(name)
                assert EXTENSION.encode() in data if name == "kwel-native-host" else True
                assert str(Path.home()).encode() not in data
                binary.write_bytes(data)
                for architecture in ("arm64", "x86_64"):
                    run("/usr/bin/lipo", binary, "-verify_arch", architecture)
    print("Alfred graph, configuration, JSON CLI scripts, permissions and Universal binaries verified.")


def verify():
    if sys.platform == "darwin":
        verify_workflow(DIST / "Kwel.alfredworkflow")
        with tempfile.TemporaryDirectory(prefix="kwel-expand-") as temporary:
            expanded = Path(temporary) / "expanded"
            run("pkgutil", "--expand-full", DIST / "KwelConnect-macos-universal.pkg", expanded)
            distribution = (expanded / "Distribution").read_text()
            assert 'enable_currentUserHome="true"' in distribution and 'enable_localSystem="false"' in distribution and 'auth="none"' in distribution
    elif sys.platform == "win32":
        installer = DIST / "KwelConnect-windows-x64-setup.exe"
        assert installer.read_bytes()[:2] == b"MZ" and installer.stat().st_size > 100_000
        for name in BINS:
            data = (DIST / "bin" / (name + ".exe")).read_bytes()
            pe = struct.unpack_from("<I", data, 0x3C)[0]
            assert data[pe:pe + 4] == b"PE\0\0" and struct.unpack_from("<H", data, pe + 4)[0] == 0x8664
    else:
        for package in DIST.glob("*.deb"):
            run("dpkg-deb", "--info", package)
            with tempfile.TemporaryDirectory(prefix="kwel-deb-verify-") as temporary:
                run("dpkg-deb", "--extract", package, temporary)
                for name in HOSTS:
                    manifest = json.loads((Path(temporary) / f"etc/opt/chrome/native-messaging-hosts/{name}.json").read_text())
                    assert manifest["name"] == name and manifest["path"] == "/usr/lib/kwel-connect/kwel-native-host"
                    assert manifest["allowed_origins"] == [f"chrome-extension://{EXTENSION}/"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("operation", choices=("build", "package", "workflow", "verify", "verify-workflow"))
    parser.add_argument("path", nargs="?")
    arguments = parser.parse_args()
    if arguments.operation in ("build", "package", "workflow"):
        build()
    if arguments.operation in ("package", "workflow") and sys.platform == "darwin":
        workflow()
    if arguments.operation == "package":
        {"darwin": macos, "win32": windows}.get(sys.platform, linux)()
    elif arguments.operation == "verify":
        verify()
    elif arguments.operation == "verify-workflow":
        verify_workflow(Path(arguments.path) if arguments.path else DIST / "Kwel.alfredworkflow")


if __name__ == "__main__":
    main()
