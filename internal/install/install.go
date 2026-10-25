package install

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
	"com.kwel/alfredbridge/internal/securefs"
)

const (
	HostName        = "com.kwel.alfred"
	HostDescription = "Kwel Alfred Integration"
	MaxControlWait  = 750 * time.Millisecond
)

var validExtensionID = regexp.MustCompile(`^[a-p]{32}$`)

type NativeManifest struct {
	Name           string   `json:"name"`
	Description    string   `json:"description"`
	Path           string   `json:"path"`
	Type           string   `json:"type"`
	AllowedOrigins []string `json:"allowed_origins"`
}

type DoctorReport struct {
	State                 string `json:"state"`
	BridgeInstalled       bool   `json:"bridgeInstalled"`
	HostRegistered        bool   `json:"hostRegistered"`
	ExtensionConnected    string `json:"extensionConnected"`
	IntegrationEnabled    string `json:"integrationEnabled"`
	NativeMessagingAccess string `json:"nativeMessagingAccess"`
	Profiles              string `json:"profiles"`
	HostVersion           string `json:"hostVersion,omitempty"`
	NextAction            string `json:"nextAction"`
}

func BinaryPath(home string) string {
	return filepath.Join(home, "Library", "Application Support", "Kwel", "AlfredBridge", "bin", "kwel-bridge")
}

func ManifestPath(home string) string {
	return filepath.Join(home, "Library", "Application Support", "Google", "Chrome", "NativeMessagingHosts", HostName+".json")
}

func legacySocketPath(home string) string {
	return filepath.Join(home, "Library", "Caches", "Kwel", "AlfredBridge", "bridge.sock")
}

func SocketDirectory() string {
	return securefs.RuntimeDirectory()
}

func EnsureSocketDirectory() (string, error) {
	directory := SocketDirectory()
	if err := securefs.EnsureRuntimePrivateDir(directory); err != nil {
		return "", err
	}
	return directory, nil
}

func ProfileSocketPath(directory, profileID string) (string, error) {
	if !protocol.ValidProfileID(profileID) {
		return "", errors.New("invalid Chrome profile identifier")
	}
	return filepath.Join(directory, strings.ToLower(profileID)+".sock"), nil
}

func ProfileSocketFiles() []string {
	return profileSocketPaths(SocketDirectory())
}

func Setup(executable, home, extensionID string) error {
	return install(executable, home, extensionID, false)
}

func install(executable, home, extensionID string, repair bool) error {
	if !validExtensionID.MatchString(extensionID) {
		return errors.New("published Kwel Chrome extension ID is not configured")
	}
	installedPath := BinaryPath(home)
	manifestPath := ManifestPath(home)
	manifestDirectory := filepath.Dir(manifestPath)
	if err := securefs.EnsureUserPrivateDir(filepath.Dir(installedPath)); err != nil {
		return fmt.Errorf("prepare private Bridge directory: %w", err)
	}
	if err := securefs.EnsureUserPrivateDir(manifestDirectory); err != nil {
		return fmt.Errorf("prepare Chrome host directory: %w", err)
	}
	manifest := NativeManifest{
		Name:           HostName,
		Description:    HostDescription,
		Path:           installedPath,
		Type:           "stdio",
		AllowedOrigins: []string{"chrome-extension://" + extensionID + "/"},
	}
	if err := checkExistingManifest(manifestPath, manifest, repair); err != nil {
		return err
	}
	source, err := filepath.EvalSymlinks(executable)
	if err != nil {
		return err
	}
	if err := securefs.VerifyOwnedRegularFile(source); err != nil {
		return fmt.Errorf("verify bundled Bridge executable: %w", err)
	}
	info, err := os.Stat(source)
	if err != nil || info.Size() < 4 {
		return errors.New("bundled Bridge executable is incomplete")
	}
	if err := securefs.AtomicCopy(source, installedPath, 0o755); err != nil {
		return fmt.Errorf("install Bridge executable: %w", err)
	}
	manifestJSON, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	manifestJSON = append(manifestJSON, '\n')
	if err := securefs.AtomicWrite(manifestPath, manifestJSON, 0o600); err != nil {
		return fmt.Errorf("register Chrome Native Messaging host: %w", err)
	}
	return nil
}

func checkExistingManifest(path string, expected NativeManifest, repair bool) error {
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	if err := securefs.VerifyOwnedRegularFile(path); err != nil {
		return fmt.Errorf("refusing to replace an unsafe Chrome host manifest: %w", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var current NativeManifest
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&current); err != nil {
		return fmt.Errorf("refusing to replace an unrecognized Chrome host manifest: %w", err)
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return errors.New("refusing to replace a Chrome host manifest with trailing data")
	}
	if current.Name != HostName || current.Description != HostDescription || current.Type != "stdio" {
		return errors.New("a different Native Messaging host already uses com.kwel.alfred; setup stopped without overwriting it")
	}
	if current.Path != expected.Path && !repair {
		return errors.New("the existing Kwel host path differs from this installation; run repair to update it")
	}
	return nil
}

func Repair(executable, home, extensionID string) error {
	return install(executable, home, extensionID, true)
}

// UpdateIfNeeded refreshes an already-installed Bridge during an explicit search.
// It does not perform first-time setup and preserves the host's macOS metadata.
func UpdateIfNeeded(executable, home, extensionID, desiredVersion string) (bool, error) {
	return updateIfNeeded(executable, home, extensionID, desiredVersion, SocketDirectory())
}

func updateIfNeeded(executable, home, extensionID, desiredVersion, socketDirectory string) (bool, error) {
	if !validExtensionID.MatchString(extensionID) {
		return false, errors.New("published Kwel Chrome extension ID is not configured")
	}
	current := doctorWithSocketDirectory(home, extensionID, socketDirectory)
	if !current.BridgeInstalled || !current.HostRegistered {
		return false, nil
	}
	if current.State == "profile_conflict" {
		return false, nil
	}
	installedPath := BinaryPath(home)
	output, err := exec.Command(installedPath, "version").Output()
	if err == nil && strings.TrimSpace(string(output)) == desiredVersion {
		return false, nil
	}
	if err := Setup(executable, home, extensionID); err != nil {
		return false, err
	}
	_ = stopActiveHosts(socketDirectory)
	_ = stopLegacyHost(home)
	return true, nil
}

func RestartConnectedHost(home string) bool {
	return StopActiveHost(home)
}

func Doctor(home, extensionID string) DoctorReport {
	return doctorWithSocketDirectory(home, extensionID, SocketDirectory())
}

func doctorWithSocketDirectory(home, extensionID, socketDirectory string) DoctorReport {
	report := DoctorReport{
		State:                 "unknown",
		ExtensionConnected:    "unknown",
		IntegrationEnabled:    "unknown",
		NativeMessagingAccess: "unknown",
		Profiles:              "unknown",
		NextAction:            "Review the Kwel and Chrome integration settings.",
	}
	binaryPath := BinaryPath(home)
	manifestPath := ManifestPath(home)
	if err := securefs.VerifyOwnedRegularFile(binaryPath); err == nil {
		report.BridgeInstalled = true
	} else if errors.Is(err, os.ErrNotExist) {
		report.State = "bridge_not_installed"
		report.NextAction = "Search Kwel from Alfred once to install the Bridge."
		return report
	} else {
		report.State = "bridge_needs_repair"
		report.NextAction = "Search Kwel again to refresh the Bridge registration."
		return report
	}
	manifest, err := readManifest(manifestPath)
	if err != nil || manifest.Name != HostName || manifest.Path != binaryPath || manifest.Type != "stdio" || !hasAllowedOrigin(manifest, extensionID) {
		report.State = "bridge_needs_repair"
		report.NextAction = "Search Kwel again to refresh the Bridge registration."
		return report
	}
	report.HostRegistered = true
	connectedCount := 0
	conflicted := false
	for _, socket := range profileSocketPaths(socketDirectory) {
		connected, profileConflict, hostVersion := checkSocket(socket)
		if profileConflict {
			conflicted = true
		}
		if connected {
			connectedCount++
			report.HostVersion = hostVersion
		}
	}
	if conflicted {
		report.State = "profile_conflict"
		report.ExtensionConnected = "connected"
		report.IntegrationEnabled = "enabled"
		report.NativeMessagingAccess = "granted"
		report.Profiles = "conflict"
		report.NextAction = "Close the duplicate Chrome connection and retry."
		return report
	}
	if connectedCount > 0 {
		report.ExtensionConnected = "connected"
		if report.HostVersion == "unknown" {
			report.State = "bridge_needs_repair"
			report.NextAction = "The running Native Messaging host could not report its version."
			return report
		}
		report.State = "ready"
		report.IntegrationEnabled = "enabled"
		report.NativeMessagingAccess = "granted"
		report.Profiles = "single"
		if connectedCount > 1 {
			report.Profiles = "multiple"
		}
		report.NextAction = "Search Kwel from Alfred."
		return report
	}
	report.State = "extension_disconnected"
	report.ExtensionConnected = "disconnected"
	report.NextAction = "Open Chrome and enable Alfred integration in Kwel Settings. If it is already enabled, check Chrome's Native Messaging permission."
	return report
}

func Uninstall(home string) error {
	manifestPath := ManifestPath(home)
	binaryPath := BinaryPath(home)
	if _, err := os.Lstat(manifestPath); err == nil {
		manifest, readErr := readManifest(manifestPath)
		if readErr != nil || manifest.Name != HostName || manifest.Description != HostDescription || manifest.Path != binaryPath {
			return errors.New("refusing to remove a Chrome Native Messaging manifest that is not owned by this Kwel Alfred Bridge")
		}
		if err := securefs.RemoveOwnedRegularFile(manifestPath); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if _, err := os.Lstat(binaryPath); err == nil {
		if err := securefs.RemoveOwnedRegularFile(binaryPath); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return nil
}

func readManifest(path string) (NativeManifest, error) {
	if err := securefs.VerifyOwnedRegularFile(path); err != nil {
		return NativeManifest{}, err
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return NativeManifest{}, err
	}
	var manifest NativeManifest
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return NativeManifest{}, err
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return NativeManifest{}, errors.New("manifest contains trailing data")
	}
	return manifest, nil
}

func hasAllowedOrigin(manifest NativeManifest, extensionID string) bool {
	return validExtensionID.MatchString(extensionID) && len(manifest.AllowedOrigins) == 1 && manifest.AllowedOrigins[0] == "chrome-extension://"+extensionID+"/"
}

func checkSocket(path string) (connected bool, conflicted bool, hostVersion string) {
	if err := securefs.VerifyPrivateDir(filepath.Dir(path)); err != nil {
		return false, false, ""
	}
	if err := securefs.VerifyOwnedSocket(path); err != nil {
		return false, false, ""
	}
	connection, err := net.DialTimeout("unix", path, 150*time.Millisecond)
	if err != nil {
		return false, false, ""
	}
	defer connection.Close()
	_ = connection.SetDeadline(time.Now().Add(MaxControlWait))
	if err := protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeDoctor}); err != nil {
		return true, false, "unknown"
	}
	payload, err := protocol.ReadFrame(connection)
	if err != nil {
		return true, false, "unknown"
	}
	response, err := protocol.Decode(payload)
	if err != nil || response.Type != protocol.TypeDoctorResult {
		return true, false, "unknown"
	}
	return true, response.Code == protocol.CodeProfileConflict, response.Text
}

func StopActiveHost(home string) bool {
	stopped := stopActiveHosts(SocketDirectory())
	return stopLegacyHost(home) || stopped
}

func stopActiveHosts(socketDirectory string) bool {
	stopped := false
	for _, path := range profileSocketPaths(socketDirectory) {
		connection, err := net.DialTimeout("unix", path, 150*time.Millisecond)
		if err != nil {
			continue
		}
		_ = connection.SetDeadline(time.Now().Add(MaxControlWait))
		writeErr := protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeUninstall})
		payload, readErr := protocol.ReadFrame(connection)
		_ = connection.Close()
		if writeErr == nil && readErr == nil {
			response, decodeErr := protocol.Decode(payload)
			if decodeErr == nil && response.Type == protocol.TypeUninstallAck {
				stopped = true
			}
		}
	}
	return stopped
}

func stopLegacyHost(home string) bool {
	path := legacySocketPath(home)
	if securefs.VerifyPrivateDir(filepath.Dir(path)) != nil || securefs.VerifyOwnedSocket(path) != nil {
		return false
	}
	connection, err := net.DialTimeout("unix", path, 150*time.Millisecond)
	if err != nil {
		return false
	}
	defer connection.Close()
	_ = connection.SetDeadline(time.Now().Add(MaxControlWait))
	if err := protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeUninstall}); err != nil {
		return false
	}
	payload, err := protocol.ReadFrame(connection)
	if err != nil {
		return false
	}
	response, err := protocol.Decode(payload)
	return err == nil && response.Type == protocol.TypeUninstallAck
}

func profileSocketPaths(directory string) []string {
	if err := securefs.VerifyPrivateDir(directory); err != nil {
		return nil
	}
	entries, err := os.ReadDir(directory)
	if err != nil {
		return nil
	}
	paths := make([]string, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".sock") {
			continue
		}
		id := strings.TrimSuffix(entry.Name(), ".sock")
		if !protocol.ValidProfileID(id) {
			continue
		}
		path := filepath.Join(directory, entry.Name())
		if securefs.VerifyOwnedSocket(path) == nil {
			paths = append(paths, path)
		}
	}
	return paths
}
