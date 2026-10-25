//go:build darwin || linux

package testutil

import (
	"os"
	"path/filepath"
	"testing"

	"com.kwel/alfredbridge/internal/securefs"
)

// SocketDir uses the test environment's temporary directory unless it would
// exceed Darwin's 104-byte Unix socket path limit. MkdirTemp creates mode 0700.
func SocketDir(t *testing.T) string {
	t.Helper()
	parent := os.TempDir()
	if len(filepath.Join(parent, "kwa-test-1234567890", "00000000-0000-4000-8000-000000000000.sock")) >= 104 {
		parent = "/tmp"
	}
	directory, err := os.MkdirTemp(parent, "kwa-test-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	if err := securefs.VerifyPrivateDir(directory); err != nil {
		t.Fatal(err)
	}
	return directory
}
