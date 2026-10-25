//go:build darwin

package securefs

import (
	"fmt"
	"os/exec"
	"strings"
)

const runtimeTempDir = "/private/tmp"

func copyPreservingMetadata(source, destination string) error {
	command := exec.Command("/usr/bin/ditto", "--rsrc", "--extattr", "--acl", source, destination)
	if output, err := command.CombinedOutput(); err != nil {
		return fmt.Errorf("copy Bridge while preserving macOS metadata: %w (%s)", err, strings.TrimSpace(string(output)))
	}
	return nil
}
