//go:build darwin || linux

package securefs

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

var ErrUnsafePath = errors.New("unsafe filesystem path")

func EnsureUserPrivateDir(path string) error {
	home, err := os.UserHomeDir()
	if err != nil {
		return err
	}
	home, err = filepath.Abs(home)
	if err != nil {
		return err
	}
	path, err = filepath.Abs(path)
	if err != nil {
		return err
	}
	if path != home && !strings.HasPrefix(path, home+string(os.PathSeparator)) {
		return fmt.Errorf("%w: directory is outside the user home", ErrUnsafePath)
	}
	if path == home {
		return checkDirectory(home, false)
	}
	relative, err := filepath.Rel(home, path)
	if err != nil {
		return err
	}
	if err := checkDirectory(home, false); err != nil {
		return err
	}
	current := home
	parts := strings.Split(relative, string(os.PathSeparator))
	for index, part := range parts {
		if part == "" || part == "." || part == ".." {
			return fmt.Errorf("%w: invalid path component", ErrUnsafePath)
		}
		current = filepath.Join(current, part)
		info, statErr := os.Lstat(current)
		if errors.Is(statErr, os.ErrNotExist) {
			if mkdirErr := os.Mkdir(current, 0o700); mkdirErr != nil && !errors.Is(mkdirErr, os.ErrExist) {
				return mkdirErr
			}
			info, statErr = os.Lstat(current)
		}
		if statErr != nil {
			return statErr
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("%w: path component is not a real directory", ErrUnsafePath)
		}
		if err := verifyOwner(info); err != nil {
			return err
		}
		if index == len(parts)-1 && info.Mode().Perm() != 0o700 {
			if err := os.Chmod(current, 0o700); err != nil {
				return err
			}
		}
	}
	return nil
}

// RuntimeDirectory is fixed per platform; callers cannot redirect production sockets.
func RuntimeDirectory() string {
	return filepath.Join(runtimeTempDir, fmt.Sprintf("kwa-%d", CurrentUID()))
}

// EnsureRuntimePrivateDir requires a root-owned sticky parent and a private child.
func EnsureRuntimePrivateDir(path string) error {
	expected := RuntimeDirectory()
	absolute, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	if absolute != expected {
		return fmt.Errorf("%w: unexpected Native Messaging runtime directory", ErrUnsafePath)
	}
	parent, err := os.Lstat(runtimeTempDir)
	if err != nil {
		return err
	}
	if err := verifyRuntimeParent(parent); err != nil {
		return err
	}
	return ensureRuntimeChild(expected)
}

func verifyRuntimeParent(parent os.FileInfo) error {
	stat, ok := parent.Sys().(*syscall.Stat_t)
	if !ok || !parent.IsDir() || parent.Mode()&os.ModeSymlink != 0 || stat.Uid != 0 || parent.Mode()&os.ModeSticky == 0 || parent.Mode().Perm() != 0o777 {
		return fmt.Errorf("%w: unsafe temporary directory", ErrUnsafePath)
	}
	return nil
}

func ensureRuntimeChild(expected string) error {
	if _, err := os.Lstat(expected); errors.Is(err, os.ErrNotExist) {
		if err := os.Mkdir(expected, 0o700); err != nil && !errors.Is(err, os.ErrExist) {
			return err
		}
	} else if err != nil {
		return err
	}
	return VerifyPrivateDir(expected)
}

func VerifyPrivateDir(path string) error {
	path, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	if home, homeErr := os.UserHomeDir(); homeErr == nil {
		home, absErr := filepath.Abs(home)
		if absErr == nil && (path == home || strings.HasPrefix(path, home+string(os.PathSeparator))) {
			current := home
			if err := checkDirectory(current, false); err != nil {
				return err
			}
			relative, err := filepath.Rel(home, path)
			if err != nil {
				return err
			}
			if relative != "." {
				for _, component := range strings.Split(relative, string(os.PathSeparator)) {
					if component == "" || component == "." || component == ".." {
						return fmt.Errorf("%w: invalid path component", ErrUnsafePath)
					}
					current = filepath.Join(current, component)
					if err := checkDirectory(current, false); err != nil {
						return err
					}
				}
			}
		}
	}
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || info.Mode().Perm() != 0o700 {
		return fmt.Errorf("%w: directory must be a non-symlink with mode 0700", ErrUnsafePath)
	}
	return verifyOwner(info)
}

func VerifyOwnedRegularFile(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("%w: expected a regular non-symlink file", ErrUnsafePath)
	}
	return verifyOwner(info)
}

func VerifyOwnedSocket(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if info.Mode()&os.ModeSocket == 0 || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("%w: expected a Unix socket", ErrUnsafePath)
	}
	return verifyOwner(info)
}

func AtomicWrite(path string, data []byte, mode os.FileMode) error {
	directory := filepath.Dir(path)
	if err := VerifyPrivateDir(directory); err != nil {
		return err
	}
	if _, err := os.Lstat(path); err == nil {
		if err := VerifyOwnedRegularFile(path); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	temporary, err := os.CreateTemp(directory, ".kwel-install-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	defer os.Remove(temporaryPath)
	if err := temporary.Chmod(mode); err != nil {
		_ = temporary.Close()
		return err
	}
	if _, err := temporary.Write(data); err != nil {
		_ = temporary.Close()
		return err
	}
	if err := temporary.Sync(); err != nil {
		_ = temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	return os.Rename(temporaryPath, path)
}

// AtomicCopy validates both paths and publishes a metadata-preserving copy atomically.
// The macOS implementation uses ditto to retain quarantine metadata.
func AtomicCopy(source, destination string, mode os.FileMode) error {
	if err := VerifyOwnedRegularFile(source); err != nil {
		return err
	}
	directory := filepath.Dir(destination)
	if err := VerifyPrivateDir(directory); err != nil {
		return err
	}
	if _, err := os.Lstat(destination); err == nil {
		if err := VerifyOwnedRegularFile(destination); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	temporary, err := os.CreateTemp(directory, ".kwel-copy-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	if err := temporary.Close(); err != nil {
		_ = os.Remove(temporaryPath)
		return err
	}
	defer os.Remove(temporaryPath)
	if err := copyPreservingMetadata(source, temporaryPath); err != nil {
		return err
	}
	if err := os.Chmod(temporaryPath, mode); err != nil {
		return err
	}
	file, err := os.OpenFile(temporaryPath, os.O_RDONLY, 0)
	if err != nil {
		return err
	}
	if err := file.Sync(); err != nil {
		_ = file.Close()
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	return os.Rename(temporaryPath, destination)
}

func RemoveOwnedRegularFile(path string) error {
	if err := VerifyOwnedRegularFile(path); err != nil {
		return err
	}
	return os.Remove(path)
}

func CurrentUID() uint32 { return uint32(os.Geteuid()) }

func checkDirectory(path string, private bool) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("%w: expected a real directory", ErrUnsafePath)
	}
	if private && info.Mode().Perm() != 0o700 {
		return fmt.Errorf("%w: directory must have mode 0700", ErrUnsafePath)
	}
	return verifyOwner(info)
}

func verifyOwner(info os.FileInfo) error {
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || uint32(stat.Uid) != CurrentUID() {
		return fmt.Errorf("%w: path is not owned by the current user", ErrUnsafePath)
	}
	return nil
}
