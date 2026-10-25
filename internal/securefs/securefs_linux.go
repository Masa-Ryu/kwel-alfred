//go:build linux

package securefs

import (
	"fmt"
	"os/exec"
	"strings"
)

const runtimeTempDir = "/tmp"

// Linux supports the shared filesystem and installation tests. It cannot verify
// macOS quarantine/resource-fork behavior; the CLI still requires macOS.
func copyPreservingMetadata(source, destination string) error {
	command := exec.Command("cp", "--preserve=all", "--no-dereference", "--", source, destination)
	if output, err := command.CombinedOutput(); err != nil {
		return fmt.Errorf("copy Bridge while preserving Linux metadata: %w (%s)", err, strings.TrimSpace(string(output)))
	}
	return nil
}
