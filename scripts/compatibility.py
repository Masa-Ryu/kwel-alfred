"""Distribution ABI and producer checks; Python standard library only."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess

MACOS_TARGETS = {"arm64": "11.0", "x86_64": "10.12"}
GLIBC_MAX = (2, 39, 0)
BUILDINFO = "BUILDINFO.json"
ARTIFACTS = {
    "macos-arm64": ("Darwin", "arm64", ("KwelConnect-macos-universal.pkg", "Kwel.alfredworkflow")),
    "windows-x64": ("Windows", "x86_64", ("KwelConnect-windows-x64-setup.exe",)),
    "ubuntu-amd64": ("Linux", "x86_64", ("KwelConnect-ubuntu-amd64.deb",)),
    "ubuntu-arm64": ("Linux", "aarch64", ("KwelConnect-ubuntu-arm64.deb",)),
}


def version(value):
    if not re.fullmatch(r"\d+\.\d+(?:\.\d+)?", value):
        raise ValueError("Invalid OS/ABI version")
    return tuple(int(part) for part in value.split(".")) + ((0,) if value.count(".") == 1 else ())


def output(*arguments):
    environment = os.environ.copy()
    environment["LC_ALL"] = "C"
    return subprocess.run([str(argument) for argument in arguments], check=True, capture_output=True, text=True, timeout=60, env=environment).stdout


def macho_minos(text):
    build = re.search(r"cmd LC_BUILD_VERSION\s+cmdsize \d+\s+platform MACOS\s+minos ([\d.]+)", text)
    legacy = re.search(r"cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version ([\d.]+)", text)
    if bool(build) == bool(legacy):
        raise ValueError("Expected one macOS deployment target")
    return (build or legacy).group(1)


def verify_macos_binary(binary):
    for architecture, minimum in MACOS_TARGETS.items():
        output("/usr/bin/lipo", binary, "-verify_arch", architecture)
        actual = macho_minos(output("/usr/bin/vtool", "-arch", architecture, "-show-build", binary))
        if version(actual) != version(minimum):
            raise ValueError(f"Unexpected {architecture} deployment target: {actual}; expected {minimum}")
        print(f"{Path(binary).name}: {architecture} deployment target {actual}")


def glibc_versions(text):
    names = set(re.findall(r"\bName:\s+(GLIBC_[A-Za-z0-9_.]+)", text))
    if not names:
        raise ValueError("No glibc version requirements were inspected")
    numbers = []
    for name in names:
        if name == "GLIBC_ABI_DT_RELR":
            numbers.append((2, 36, 0))
        else:
            numbers.append(version(name.removeprefix("GLIBC_")))
    if max(numbers) > GLIBC_MAX:
        raise ValueError("Binary requires a glibc version newer than Ubuntu 24.04's 2.39")
    return max(numbers)


def verify_linux_binary(binary, architecture):
    with Path(binary).open("rb") as stream:
        header = stream.read(64)
    expected = {"amd64": 62, "arm64": 183}[architecture]
    if header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != expected:
        raise ValueError("Unexpected ELF architecture")
    reader = shutil.which("readelf") or shutil.which("llvm-readelf")
    if not reader:
        raise ValueError("readelf is required for distribution ABI verification")
    maximum = glibc_versions(output(reader, "--wide", "--version-info", binary))
    needed = re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", output(reader, "--wide", "--dynamic", binary))
    allowed = {"libc.so.6", "libgcc_s.so.1", "libm.so.6", "libpthread.so.0", "librt.so.1", "libdl.so.2", "libresolv.so.2", "libutil.so.1", "ld-linux-x86-64.so.2", "ld-linux-aarch64.so.1"}
    if not needed or set(needed) - allowed:
        raise ValueError("ELF dependencies are not covered by libc6/libgcc-s1")
    print(f"{Path(binary).name}: {architecture}, maximum required GLIBC_{'.'.join(map(str, maximum))}")


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_buildinfo(directory, distribution_version, files):
    system = platform.system()
    release = platform.freedesktop_os_release() if system == "Linux" else {}
    info = {
        "schema": 1,
        "version": distribution_version,
        "system": system,
        "machine": {"AMD64": "x86_64"}.get(platform.machine(), platform.machine()),
        "osId": release.get("ID", ""),
        "osVersion": release.get("VERSION_ID", platform.mac_ver()[0] if system == "Darwin" else platform.version()),
        "sha256": {name: digest(Path(directory) / name) for name in files},
    }
    (Path(directory) / BUILDINFO).write_text(json.dumps(info, indent=2) + "\n")


def verify_artifact(directory, artifact, distribution_version):
    directory = Path(directory)
    metadata = directory / BUILDINFO
    if metadata.is_symlink() or metadata.stat().st_size > 65536:
        raise ValueError("Invalid build provenance")
    info = json.loads(metadata.read_text())
    system, machine, files = ARTIFACTS[artifact]
    if set(info) != {"schema", "version", "system", "machine", "osId", "osVersion", "sha256"}:
        raise ValueError("Invalid build provenance fields")
    if (info["schema"], info["version"], info["system"], info["machine"]) != (1, distribution_version, system, machine):
        raise ValueError("Artifact producer does not match its release role")
    if system == "Linux" and (info["osId"], info["osVersion"]) != ("ubuntu", "24.04"):
        raise ValueError("Linux release artifacts must originate on Ubuntu 24.04")
    if system == "Darwin" and version(info["osVersion"])[0] != 26:
        raise ValueError("macOS release artifacts must originate on macOS 26")
    if set(info["sha256"]) != set(files):
        raise ValueError("Unexpected artifact contents")
    for name in files:
        path = directory / name
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0 or digest(path) != info["sha256"][name]:
            raise ValueError("Artifact checksum differs from its producer manifest")
    print(f"{artifact}: verified {system} {info['osVersion']} release producer and checksums")
