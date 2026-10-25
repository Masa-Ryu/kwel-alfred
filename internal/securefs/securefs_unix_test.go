//go:build darwin || linux

package securefs

import (
	"errors"
	"net"
	"os"
	"path/filepath"
	"syscall"
	"testing"
)

func TestPrivateDirectoriesRejectEscapesAndSymlinkAncestors(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	directory := filepath.Join(home, "Library", "Kwel")
	if err := EnsureUserPrivateDir(directory); err != nil {
		t.Fatal(err)
	}
	if err := VerifyPrivateDir(directory); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(directory, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := VerifyPrivateDir(directory); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a non-private directory: %v", err)
	}
	if err := EnsureUserPrivateDir(directory); err != nil {
		t.Fatal(err)
	}
	if err := VerifyPrivateDir(directory); err != nil {
		t.Fatalf("did not restore private permissions: %v", err)
	}
	outside := t.TempDir()
	if err := os.Symlink(outside, filepath.Join(home, "linked")); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{outside, filepath.Join(home, "linked", "child")} {
		if err := EnsureUserPrivateDir(path); !errors.Is(err, ErrUnsafePath) {
			t.Fatalf("accepted an escaped or symlinked path %q: %v", path, err)
		}
	}
	if err := VerifyPrivateDir(filepath.Join(home, "linked")); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a symlink directory: %v", err)
	}
}

func TestAtomicWritesAndCopiesKeepPrivateModesAndRejectSymlinks(t *testing.T) {
	directory := t.TempDir()
	if err := os.Chmod(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(directory, "source")
	destination := filepath.Join(directory, "destination")
	if err := os.WriteFile(source, []byte("original"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := AtomicCopy(source, destination, 0o755); err != nil {
		t.Fatal(err)
	}
	assertFile(t, destination, "original", 0o755)
	if err := AtomicWrite(destination, []byte("updated"), 0o600); err != nil {
		t.Fatal(err)
	}
	assertFile(t, destination, "updated", 0o600)
	link := filepath.Join(directory, "link")
	if err := os.Symlink(source, link); err != nil {
		t.Fatal(err)
	}
	for name, operation := range map[string]func() error{
		"copy source":      func() error { return AtomicCopy(link, destination, 0o755) },
		"copy destination": func() error { return AtomicCopy(source, link, 0o755) },
		"write":            func() error { return AtomicWrite(link, []byte("unsafe"), 0o600) },
		"remove":           func() error { return RemoveOwnedRegularFile(link) },
	} {
		if err := operation(); !errors.Is(err, ErrUnsafePath) {
			t.Fatalf("%s accepted a symlink: %v", name, err)
		}
	}
	assertFile(t, source, "original", 0o755)
	assertFile(t, destination, "updated", 0o600)
	if err := os.Chmod(directory, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := AtomicWrite(destination, []byte("unsafe"), 0o600); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("write accepted a non-private parent: %v", err)
	}
	if err := AtomicCopy(source, destination, 0o755); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("copy accepted a non-private parent: %v", err)
	}
	assertFile(t, destination, "updated", 0o600)
}

func TestSocketAndRegularFileTypesAreNotInterchangeable(t *testing.T) {
	// Keep the socket short on macOS as well as Linux.
	directory, err := os.MkdirTemp("", "kw-fs-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	path := filepath.Join(directory, "s")
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	if err := VerifyOwnedSocket(path); err != nil {
		t.Fatal(err)
	}
	if err := VerifyOwnedRegularFile(path); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a socket as a regular file: %v", err)
	}
	file := filepath.Join(directory, "file")
	if err := os.WriteFile(file, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := VerifyOwnedSocket(file); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a regular file as a socket: %v", err)
	}
}

type testFileInfo struct {
	os.FileInfo
	mode os.FileMode
	stat any
}

func (info testFileInfo) Mode() os.FileMode { return info.mode }
func (info testFileInfo) IsDir() bool       { return info.mode.IsDir() }
func (info testFileInfo) Sys() any          { return info.stat }

func TestOwnershipAndStickyParentChecksFailClosed(t *testing.T) {
	for _, stat := range []any{nil, &syscall.Stat_t{Uid: CurrentUID() + 1}} {
		if err := verifyOwner(testFileInfo{stat: stat}); !errors.Is(err, ErrUnsafePath) {
			t.Fatalf("accepted unknown or foreign ownership: %v", err)
		}
	}
	if err := verifyOwner(testFileInfo{stat: &syscall.Stat_t{Uid: CurrentUID()}}); err != nil {
		t.Fatal(err)
	}
	mode := os.ModeDir | os.ModeSticky | 0o777
	if err := verifyRuntimeParent(testFileInfo{mode: mode, stat: &syscall.Stat_t{Uid: 0}}); err != nil {
		t.Fatal(err)
	}
	for _, info := range []testFileInfo{
		{mode: mode, stat: &syscall.Stat_t{Uid: 1}},
		{mode: mode, stat: nil},
		{mode: os.ModeDir | 0o777, stat: &syscall.Stat_t{Uid: 0}},
		{mode: os.ModeDir | os.ModeSticky | 0o755, stat: &syscall.Stat_t{Uid: 0}},
		{mode: mode | os.ModeSymlink, stat: &syscall.Stat_t{Uid: 0}},
	} {
		if err := verifyRuntimeParent(info); !errors.Is(err, ErrUnsafePath) {
			t.Fatalf("accepted unsafe runtime parent: %#v, %v", info, err)
		}
	}
}

func TestRuntimeDirectoryRejectsRedirectionAndUnsafeExistingChildren(t *testing.T) {
	path := filepath.Join(t.TempDir(), "runtime")
	if err := EnsureRuntimePrivateDir(path); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a redirected runtime directory: %v", err)
	}
	if err := ensureRuntimeChild(path); err != nil {
		t.Fatal(err)
	}
	if err := VerifyPrivateDir(path); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := ensureRuntimeChild(path); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a pre-existing public runtime directory: %v", err)
	}
	link := filepath.Join(t.TempDir(), "runtime")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if err := ensureRuntimeChild(link); !errors.Is(err, ErrUnsafePath) {
		t.Fatalf("accepted a symlink runtime directory: %v", err)
	}
}

func assertFile(t *testing.T, path, content string, mode os.FileMode) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil || string(data) != content {
		t.Fatalf("file %q = %q, %v", path, data, err)
	}
	info, err := os.Lstat(path)
	if err != nil || info.Mode().Perm() != mode {
		t.Fatalf("file %q has unexpected permissions: %v, %v", path, info, err)
	}
}
