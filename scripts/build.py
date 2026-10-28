#!/usr/bin/env python3
"""Build and verify current-platform packages without reading user data."""
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
from compatibility import MACOS_TARGETS, verify_artifact, verify_linux_binary, verify_macos_binary, write_buildinfo

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
EXTENSION = json.loads((ROOT / "config/distribution.json").read_text())["chromeExtensionId"]
HOST_NAME = "com.kwel.bridge"
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
            target_environment = environment.copy()
            target_environment["MACOSX_DEPLOYMENT_TARGET"] = MACOS_TARGETS["arm64" if target.startswith("aarch64") else "x86_64"]
            run("cargo", "build", "--workspace", "--release", "--locked", "--target", target, cwd=ROOT, env=target_environment)
        for name in BINS:
            run("lipo", "-create", *(ROOT / "target" / target / "release" / name for target in ("aarch64-apple-darwin", "x86_64-apple-darwin")), "-output", destination / name)
            verify_macos_binary(destination / name)
            (destination / name).chmod(0o700)
    else:
        if sys.platform == "win32":
            environment["RUSTFLAGS"] += " -C target-feature=+crt-static"
            # An explicit target keeps crt-static away from host proc-macros.
            target = "x86_64-pc-windows-msvc"
            run("cargo", "build", "--workspace", "--release", "--locked", "--target", target, cwd=ROOT, env=environment)
            source = ROOT / "target" / target / "release"
        else:
            run("cargo", "build", "--workspace", "--release", "--locked", cwd=ROOT, env=environment)
            source = ROOT / "target/release"
        for name in BINS:
            filename = name + (".exe" if sys.platform == "win32" else "")
            shutil.copyfile(source / filename, destination / filename)
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
        (control / "control").write_text((ROOT / "packaging/linux/control.in").read_text().replace("@VERSION@", VERSION).replace("@ARCH@", architecture))
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
        name = HOST_NAME
        (manifests / f"{name}.json").write_text(json.dumps({"name": name, "description": "Kwel Local Connection", "path": "/usr/lib/kwel-connect/kwel-native-host", "type": "stdio", "allowed_origins": [f"chrome-extension://{EXTENSION}/"]}, indent=2) + "\n")
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
        assert info == plistlib.loads((ROOT / "workflow/alfred/info.plist").read_bytes()), "Packaged Workflow metadata differs from source"
        assert info["bundleid"] == "com.kwel.alfred.search" and info["version"] == VERSION
        field, = info["userconfigurationconfig"]
        assert field == {
            "config": {"default": "", "placeholder": "", "required": True, "trim": True},
            "description": (
                "Paste the Profile ID from Kwel Settings → Local Connection. "
                "On macOS, unsigned executables may require approval on first use. "
                "See the Workflow Readme for instructions."
            ),
            "label": "Kwel Profile ID",
            "type": "textfield",
            "variable": "KWEL_PROFILE_ID",
        }
        assert not info.get("variables", {}).get("KWEL_PROFILE_ID")
        assert info["webaddress"] == "https://kwel.ryu.zone"
        assert "github.com/Masa-Ryu/kwel-connect/releases" in info["readme"]
        for text in (
            "## macOS First Launch", "unsigned and not notarized", *BINS,
            "official GitHub Releases", "published SHA-256 checksums",
            "System Settings → Privacy & Security → Open Anyway",
            "Each executable may require a separate confirmation.",
            "do not approve it; check the exact warning text.",
            "Do not disable macOS security protections.",
            "https://support.apple.com/en-us/102445",
        ):
            assert text in info["readme"], f"Missing Workflow first-launch guidance: {text}"
        readme = archive.read("README.md")
        assert readme == (ROOT / "README.md").read_bytes(), "Packaged README differs from source"
        assert all(f"`{name}`".encode() in readme for name in BINS)
        objects = info["objects"]
        assert len(objects) == 4
        search = next(value for value in objects if value["type"] == "alfred.workflow.input.scriptfilter")
        fallback = next(value for value in objects if value["type"] == "alfred.workflow.trigger.fallback")
        assert fallback["config"]["text"] == "Search Kwel"
        assert not search["config"]["keyword"] and not search["config"]["alfredfiltersresults"]
        assert "inboundconfig" not in search["config"] and search["config"]["scriptargtype"] == 1
        assert search["config"]["scriptfile"] == "./scripts/search.sh"
        assert len(info["connections"]) == 3
        for name in ("search.sh", "open-result.sh"):
            assert archive.read(f"scripts/{name}") == (ROOT / "workflow/alfred/scripts" / name).read_bytes()
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
                verify_macos_binary(binary)
    print("Alfred first-launch guidance, graph, configuration, JSON CLI scripts, permissions and Universal binaries verified.")


def pe_dependencies(data, pe):
    """Read PE32+ import names so release checks verify actual DLL dependencies."""
    optional = pe + 24
    assert struct.unpack_from("<H", data, optional)[0] == 0x20B
    sections = optional + struct.unpack_from("<H", data, pe + 20)[0]
    count = struct.unpack_from("<H", data, pe + 6)[0]

    def file_offset(rva):
        for index in range(count):
            virtual_size, address, size, offset = struct.unpack_from("<IIII", data, sections + 40 * index + 8)
            if address <= rva < address + max(virtual_size, size):
                assert rva - address < size
                return offset + rva - address
        raise ValueError("Invalid PE import address")

    address, size = struct.unpack_from("<II", data, optional + 120)
    assert address and size >= 20
    start = file_offset(address)
    names = []
    for index in range(min(size // 20, 256)):
        descriptor = struct.unpack_from("<IIIII", data, start + 20 * index)
        if not any(descriptor):
            return names
        offset = file_offset(descriptor[3])
        end = data.index(b"\0", offset, offset + 256)
        names.append(data[offset:end].decode("ascii").lower())
    raise ValueError("Unterminated PE import directory")


def verify_linux_package(package):
    architecture = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
    assert output_field(package, "Architecture") == architecture
    assert output_field(package, "Version") == VERSION
    assert output_field(package, "Depends") == "libc6 (>= 2.39), libgcc-s1"
    run("dpkg-deb", "--info", package)
    with tempfile.TemporaryDirectory(prefix="kwel-deb-verify-") as temporary:
        stage = Path(temporary)
        run("dpkg-deb", "--extract", package, stage)
        run("dpkg-deb", "--control", package, stage / "DEBIAN")
        name = HOST_NAME
        assert {path.name for path in (stage / "etc/opt/chrome/native-messaging-hosts").iterdir()} == {f"{name}.json"}
        manifest = json.loads((stage / f"etc/opt/chrome/native-messaging-hosts/{name}.json").read_text())
        assert manifest["name"] == name and manifest["path"] == "/usr/lib/kwel-connect/kwel-native-host"
        assert manifest["allowed_origins"] == [f"chrome-extension://{EXTENSION}/"]
        binaries = ("usr/lib/kwel-connect/kwel-native-host", "usr/bin/kwel-bridge", "DEBIAN/preinst", "DEBIAN/prerm")
        for name in binaries:
            binary = stage / name
            assert binary.is_file() and not binary.is_symlink() and stat.S_IMODE(binary.stat().st_mode) == 0o755
            verify_linux_binary(binary, architecture)
        host = (stage / binaries[0]).read_bytes()
        assert all((stage / name).read_bytes() == host for name in ("DEBIAN/preinst", "DEBIAN/prerm"))


def output_field(package, field):
    return run("dpkg-deb", "--field", package, field, capture_output=True, text=True).stdout.strip()


def verify(directory=DIST):
    directory = Path(directory)
    if sys.platform == "darwin":
        verify_workflow(directory / "Kwel.alfredworkflow")
        with tempfile.TemporaryDirectory(prefix="kwel-expand-") as temporary:
            expanded = Path(temporary) / "expanded"
            run("pkgutil", "--expand-full", directory / "KwelConnect-macos-universal.pkg", expanded)
            distribution = (expanded / "Distribution").read_text()
            assert 'enable_currentUserHome="true"' in distribution and 'enable_localSystem="false"' in distribution and 'auth="none"' in distribution
            payload = expanded / "component.pkg/Payload/Library/Application Support/Kwel/NativeBridge/installer-payload"
            for name in BINS[:2]:
                verify_macos_binary(payload / name)
    elif sys.platform == "win32":
        installer = directory / "KwelConnect-windows-x64-setup.exe"
        assert installer.read_bytes()[:2] == b"MZ" and installer.stat().st_size > 100_000
        for name in BINS:
            data = (directory / "bin" / (name + ".exe")).read_bytes()
            pe = struct.unpack_from("<I", data, 0x3C)[0]
            assert data[pe:pe + 4] == b"PE\0\0" and struct.unpack_from("<H", data, pe + 4)[0] == 0x8664
            assert not any(name.startswith(("vcruntime", "msvcp")) for name in pe_dependencies(data, pe)), "A separate Visual C++ runtime must not be required"
    else:
        architecture = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
        package = directory / f"KwelConnect-ubuntu-{architecture}.deb"
        assert package.is_file()
        verify_linux_package(package)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("operation", choices=("build", "package", "workflow", "verify", "verify-workflow", "verify-artifact"))
    parser.add_argument("path", nargs="?")
    parser.add_argument("--artifact-name")
    arguments = parser.parse_args()
    if arguments.operation in ("build", "package", "workflow"):
        build()
    if arguments.operation in ("package", "workflow") and sys.platform == "darwin":
        workflow()
    if arguments.operation == "package":
        {"darwin": macos, "win32": windows}.get(sys.platform, linux)()
        files = {"darwin": ["KwelConnect-macos-universal.pkg", "Kwel.alfredworkflow"], "win32": ["KwelConnect-windows-x64-setup.exe"]}.get(sys.platform)
        if files is None:
            architecture = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
            files = [f"KwelConnect-ubuntu-{architecture}.deb"]
        write_buildinfo(DIST, VERSION, files)
    elif arguments.operation == "verify":
        verify(Path(arguments.path) if arguments.path else DIST)
    elif arguments.operation == "verify-workflow":
        verify_workflow(Path(arguments.path) if arguments.path else DIST / "Kwel.alfredworkflow")
    elif arguments.operation == "verify-artifact":
        if not arguments.path or not arguments.artifact_name:
            parser.error("verify-artifact requires a directory and --artifact-name")
        verify_artifact(arguments.path, arguments.artifact_name, VERSION)


if __name__ == "__main__":
    main()
