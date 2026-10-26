#!/usr/bin/env python3
"""Exercise real installers only in disposable GitHub-hosted user accounts."""
import argparse
import json
import os
from pathlib import Path
import platform
import queue
import struct
import subprocess
import sys
import threading
import tempfile
import tomllib
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
HOSTS = ("com.kwel.bridge", "com.kwel.alfred")
ORIGIN = "chrome-extension://mhablbmgafbdofgaffhdnnknjhafmlin/"


def run(*arguments, **kwargs):
    return subprocess.run([str(value) for value in arguments], check=True, timeout=180, **kwargs)


def manifests(directory, binary):
    for name in HOSTS:
        value = json.loads((directory / f"{name}.json").read_text())
        assert value["name"] == name and Path(value["path"]) == binary and value["type"] == "stdio"
        assert value["allowed_origins"] == [ORIGIN]


def installed_protocol(host_binary, client_binary, workflow_cli=None):
    """Actual installed processes/IPC; a synthetic peer replaces Chrome only."""
    profile, reference = str(uuid.uuid4()), str(uuid.uuid4())
    host = subprocess.Popen([str(host_binary), ORIGIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    frames = queue.Queue(maxsize=8)

    def read_exact(length):
        result = bytearray()
        while len(result) < length:
            data = host.stdout.read(length - len(result))
            if not data:
                raise EOFError("Native Host output ended")
            result.extend(data)
        return result

    def read_frames():
        try:
            while True:
                length, = struct.unpack("<I", read_exact(4))
                assert 0 < length <= 1_000_000
                frames.put(json.loads(read_exact(length)), timeout=5)
        except Exception as error:
            frames.put(error, timeout=5)

    reader = threading.Thread(target=read_frames, daemon=True)
    reader.start()

    def send(message):
        payload = json.dumps(message, separators=(",", ":")).encode()
        host.stdin.write(struct.pack("<I", len(payload)) + payload)
        host.stdin.flush()

    def receive(kind):
        message = frames.get(timeout=8)
        if isinstance(message, Exception):
            raise message
        assert message["type"] == kind and message["v"] == 1
        return message

    def client(*arguments):
        return subprocess.Popen([str(client_binary), *arguments, "--profile-id", profile, "--format", "json"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def complete(process, successful=True):
        output, diagnostic = process.communicate(timeout=18)
        assert (process.returncode == 0) == successful
        assert not diagnostic
        return json.loads(output)

    try:
        send({"v": 1, "type": "hello", "profileId": profile, "profileLabel": "Installer fixture"})
        receive("hello-ack")
        probing = client("profile-probe")
        receive("ping")
        send({"v": 1, "type": "pong"})
        probe = complete(probing)
        assert probe["profileId"] == profile and probe["hostVersion"] == VERSION
        searching = client("search", "--query", "Kyoto", "--limit", "20")
        request = receive("search")
        assert request["query"] == "Kyoto" and request["limit"] == 20
        items = [{"title": "Kyoto fixture", "url": "https://example.com/kyoto", "domain": "example.com", "openRef": reference}]
        send({"v": 1, "type": "search-result", "requestId": request["requestId"], "items": items, "hasMore": False})
        assert complete(searching)["items"] == items
        opening = client("open", "--ref", reference)
        request = receive("open-page")
        assert request["openRef"] == reference and "url" not in request
        send({"v": 1, "type": "open-result", "requestId": request["requestId"], "ok": True})
        assert complete(opening)["ok"] is True
        assert complete(client("open", "--ref", reference), successful=False)["code"] == "RESULT_EXPIRED"
        empty = client("search", "--query", "", "--limit", "20")
        request = receive("search")
        assert request["query"] == "" and "profileId" not in request
        # These valid notices must neither terminate the installed Host nor
        # complete the pending search. The following response is its only reply.
        for code in ("BAD_REQUEST", "INCOMPATIBLE_VERSION", "NOT_READY", "SEARCH_FAILED", "RESULT_EXPIRED", "OPEN_FAILED"):
            send({"v": 1, "type": "error", "requestId": None, "code": code})
        send({"v": 1, "type": "search-result", "requestId": request["requestId"], "items": [], "hasMore": False})
        assert complete(empty)["items"] == []
        if workflow_cli:
            alfred = subprocess.Popen([str(workflow_cli), "search", "--profile-id", profile, "--query", "Kyoto", "--limit", "20", "--format", "alfred"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            request = receive("search")
            send({"v": 1, "type": "search-result", "requestId": request["requestId"], "items": items, "hasMore": False})
            result = complete(alfred)
            assert result["skipknowledge"] is True and result["items"][0]["arg"] == reference
            assert result["items"][0]["variables"]["KWEL_RESULT_PROFILE_ID"] == profile
            assert "uid" not in result["items"][0]
    finally:
        host.stdin.close()
        try:
            assert host.wait(timeout=8) == 0
        except subprocess.TimeoutExpired:
            host.kill()
            host.wait()
            raise
        reader.join(timeout=2)
    print("Installed Native framing, profile probe, search, open and single-use IPC verified.")


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Installer tests require a disposable GitHub-hosted runner.")
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=Path, default=ROOT / "dist")
    arguments = parser.parse_args()
    artifacts = arguments.artifact_dir.resolve()
    if sys.platform == "darwin":
        package = artifacts / "KwelConnect-macos-universal.pkg"
        domains = run("installer", "-dominfo", "-pkg", package, capture_output=True, text=True).stdout
        assert "CurrentUserHomeDirectory" in domains
        root = Path.home() / "Library/Application Support/Kwel/NativeBridge"
        chrome = Path.home() / "Library/Application Support/Google/Chrome/NativeMessagingHosts"
        for _ in range(2):
            run("installer", "-pkg", package, "-target", "CurrentUserHomeDirectory")
        manifests(chrome, root / "bin/kwel-native-host")
        with tempfile.TemporaryDirectory(prefix="kwel-alfred-compat-") as temporary:
            workflow = Path(temporary).resolve()
            with zipfile.ZipFile(artifacts / "Kwel.alfredworkflow") as archive:
                for name in ("kwel-native-host", "kwel-bridge", "kwel-alfred"):
                    binary = workflow / name
                    binary.write_bytes(archive.read(name))
                    binary.chmod(0o700)
            installed_protocol(root / "bin/kwel-native-host", root / "bin/kwel-bridge", workflow / "kwel-alfred")
        run(root / "bin/kwel-native-host", "uninstall")
        assert not (root / "bin/kwel-native-host").exists()
        for name in HOSTS:
            assert not (chrome / f"{name}.json").exists()
    elif sys.platform == "win32":
        import winreg
        root = Path(os.environ["LOCALAPPDATA"]) / "Kwel/NativeBridge"
        for _ in range(2):
            run(artifacts / "KwelConnect-windows-x64-setup.exe", "/S")
        manifests(root / "manifests", root / "bin/kwel-native-host.exe")
        for name in HOSTS:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, rf"Software\Google\Chrome\NativeMessagingHosts\{name}") as key:
                assert Path(winreg.QueryValueEx(key, "")[0]) == root / "manifests" / f"{name}.json"
        installed_protocol(root / "bin/kwel-native-host.exe", root / "bin/kwel-bridge.exe")
        run(root / "Uninstall.exe", "/S", f"_?={root}")
        assert not (root / "bin/kwel-native-host.exe").exists()
        for name in HOSTS:
            try:
                with winreg.OpenKey(winreg.HKEY_CURRENT_USER, rf"Software\Google\Chrome\NativeMessagingHosts\{name}"):
                    raise AssertionError("Native Host registry entry remains")
            except FileNotFoundError:
                pass
    else:
        architecture = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
        package = artifacts / f"KwelConnect-ubuntu-{architecture}.deb"
        for _ in range(2):
            run("sudo", "dpkg", "--install", package)
        manifests(Path("/etc/opt/chrome/native-messaging-hosts"), Path("/usr/lib/kwel-connect/kwel-native-host"))
        installed_protocol(Path("/usr/lib/kwel-connect/kwel-native-host"), Path("/usr/bin/kwel-bridge"))
        run("sudo", "dpkg", "--remove", "kwel-connect")
        assert not Path("/usr/lib/kwel-connect/kwel-native-host").exists()
        for name in HOSTS:
            assert not Path(f"/etc/opt/chrome/native-messaging-hosts/{name}.json").exists()
    print("Installer registration, idempotent upgrade and owned removal verified.")


if __name__ == "__main__":
    main()
