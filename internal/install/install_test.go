package install

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

const extensionIDForTest = "mhablbmgafbdofgaffhdnnknjhafmlin"

func TestSetupIsIdempotentAndRegistersPublishedOrigin(t *testing.T) {
	home := t.TempDir()
	if err := os.Chmod(home, 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	source := filepath.Join(home, "workflow", "kwel-bridge")
	if err := os.MkdirAll(filepath.Dir(source), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(source, []byte("bridge executable test fixture"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := Setup(source, home, extensionIDForTest); err != nil {
		t.Fatal(err)
	}
	manifestData, err := os.ReadFile(ManifestPath(home))
	if err != nil {
		t.Fatal(err)
	}
	var manifest NativeManifest
	if err := json.Unmarshal(manifestData, &manifest); err != nil {
		t.Fatal(err)
	}
	if manifest.Name != HostName || manifest.Path != BinaryPath(home) || manifest.Type != "stdio" {
		t.Fatalf("unexpected host manifest: %#v", manifest)
	}
	if len(manifest.AllowedOrigins) != 1 || manifest.AllowedOrigins[0] != "chrome-extension://"+extensionIDForTest+"/" {
		t.Fatalf("allowed origins are not exact: %#v", manifest.AllowedOrigins)
	}
	if err := Setup(source, home, extensionIDForTest); err != nil {
		t.Fatalf("second setup was not idempotent: %v", err)
	}
	// A Workflow build for the installed Kwel extension must replace the old
	// owned registration's origin without retaining access for the old build.
	updatedExtensionID := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	if err := Repair(source, home, updatedExtensionID); err != nil {
		t.Fatalf("could not update the owned host origin: %v", err)
	}
	updatedManifest, err := readManifest(ManifestPath(home))
	if err != nil || !hasAllowedOrigin(updatedManifest, updatedExtensionID) || hasAllowedOrigin(updatedManifest, extensionIDForTest) {
		t.Fatalf("host origin did not migrate exclusively: %#v, %v", updatedManifest, err)
	}
	if err := Repair(source, home, extensionIDForTest); err != nil {
		t.Fatal(err)
	}
	report := doctorWithSocketDirectory(home, extensionIDForTest, t.TempDir())
	if report.State != "extension_disconnected" || !report.BridgeInstalled || !report.HostRegistered {
		t.Fatalf("doctor report should distinguish installed from connected: %#v", report)
	}
	if report.IntegrationEnabled != "unknown" || report.NativeMessagingAccess != "unknown" {
		t.Fatalf("doctor must not infer unavailable state: %#v", report)
	}
	if err := Uninstall(home); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(BinaryPath(home)); !os.IsNotExist(err) {
		t.Fatalf("Bridge binary remains after uninstall: %v", err)
	}
	if _, err := os.Lstat(ManifestPath(home)); !os.IsNotExist(err) {
		t.Fatalf("host manifest remains after uninstall: %v", err)
	}
}

func TestSetupRefusesConflictingAndSymlinkManifests(t *testing.T) {
	home := t.TempDir()
	if err := os.Chmod(home, 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	source := filepath.Join(home, "workflow", "kwel-bridge")
	if err := os.MkdirAll(filepath.Dir(source), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(source, []byte("bridge executable test fixture"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := Setup(source, home, extensionIDForTest); err != nil {
		t.Fatal(err)
	}
	manifestPath := ManifestPath(home)
	manifest, err := readManifest(manifestPath)
	if err != nil {
		t.Fatal(err)
	}
	manifest.Path = filepath.Join(home, "other-app")
	data, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(manifestPath, data, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := Setup(source, home, extensionIDForTest); err == nil {
		t.Fatal("setup overwrote a conflicting manifest")
	}
	if err := Repair(source, home, extensionIDForTest); err != nil {
		t.Fatalf("repair did not restore this Bridge's old manifest: %v", err)
	}
	manifest, err = readManifest(manifestPath)
	if err != nil || manifest.Path != BinaryPath(home) {
		t.Fatalf("repair did not restore the installed path: %#v %v", manifest, err)
	}
	if err := os.Remove(manifestPath); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(home, "outside"), manifestPath); err != nil {
		t.Fatal(err)
	}
	if err := Setup(source, home, extensionIDForTest); err == nil {
		t.Fatal("setup followed a symlink manifest")
	}
}

func TestSearchUpdatesAnAlreadyInstalledBridgeAfterWorkflowUpgrade(t *testing.T) {
	home := t.TempDir()
	if err := os.Chmod(home, 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	oldBinary := filepath.Join(home, "workflow-old", "kwel-bridge")
	newBinary := filepath.Join(home, "workflow-new", "kwel-bridge")
	writeVersionFixture(t, oldBinary, "0.1.0")
	writeVersionFixture(t, newBinary, "0.2.0")
	if err := Setup(oldBinary, home, extensionIDForTest); err != nil {
		t.Fatal(err)
	}
	updated, err := updateIfNeeded(newBinary, home, extensionIDForTest, "0.2.0", t.TempDir())
	if err != nil || !updated {
		t.Fatalf("Bridge update = %t, %v", updated, err)
	}
	output, err := exec.Command(BinaryPath(home), "version").Output()
	if err != nil || string(output) != "0.2.0\n" {
		t.Fatalf("installed executable version = %q, %v", output, err)
	}
	updated, err = updateIfNeeded(newBinary, home, extensionIDForTest, "0.2.0", t.TempDir())
	if err != nil || updated {
		t.Fatalf("matching Bridge was reinstalled: %t, %v", updated, err)
	}
}

func writeVersionFixture(t *testing.T, path, version string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	script := []byte("#!/bin/sh\nprintf '%s\\n' '" + version + "'\n")
	if err := os.WriteFile(path, script, 0o755); err != nil {
		t.Fatal(err)
	}
}
