package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"com.kwel/alfredbridge/internal/alfred"
	"com.kwel/alfredbridge/internal/install"
	"com.kwel/alfredbridge/internal/protocol"
	"com.kwel/alfredbridge/internal/relay"
	"com.kwel/alfredbridge/internal/securefs"
)

var (
	allowedExtensionID = ""
	version            = "1.00"
	kwelClientSupport  = "false"
)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "kwel-bridge:", safeError(err))
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) > 0 && strings.HasPrefix(args[0], "chrome-extension://") {
		if runtime.GOOS != "darwin" {
			return errors.New("Native Messaging is supported only on macOS")
		}
		socketDirectory, err := install.EnsureSocketDirectory()
		if err != nil {
			return err
		}
		return relay.RunNativeHost(os.Stdin, os.Stdout, args[0], allowedExtensionID, version, socketDirectory)
	}
	if len(args) == 0 {
		return usageError()
	}
	if runtime.GOOS != "darwin" {
		return errors.New("Kwel Alfred Bridge is built for macOS")
	}
	switch args[0] {
	case "search":
		return search(args[1:])
	case "open":
		return openResult(args[1:])
	case "uninstall":
		return uninstall(args[1:])
	case "version":
		fmt.Println(version)
		return nil
	default:
		return usageError()
	}
}

func search(args []string) error {
	set := flag.NewFlagSet("search", flag.ContinueOnError)
	set.SetOutput(io.Discard)
	query := set.String("query", "", "Search words")
	limit := set.Int("limit", 20, "Maximum result count")
	format := set.String("format", "alfred", "Output format")
	profileID := set.String("profile-id", "", "Configured Kwel Profile ID")
	if err := set.Parse(args); err != nil || set.NArg() != 0 || *format != "alfred" {
		return writeAlfred(alfred.Notice("Kwel search could not start."))
	}
	configuredProfileID := strings.ToLower(strings.TrimSpace(*profileID))
	if !protocol.ValidProfileID(configuredProfileID) {
		return writeAlfred(alfred.ProfileConfigurationRequired())
	}
	if err := protocol.ValidateSearch(protocol.Message{V: protocol.Version, Type: protocol.TypeSearch, RequestID: "validation", Query: *query, Limit: *limit}); err != nil {
		return writeAlfred(alfred.Notice("Enter a search query of up to 200 characters."))
	}
	if kwelClientSupport != "true" {
		return writeAlfred(alfred.Notice("This Kwel extension build does not support Alfred integration."))
	}

	home, err := os.UserHomeDir()
	if err != nil {
		return writeAlfred(alfred.Notice("Kwel could not read the current user profile."))
	}
	executable, executableErr := os.Executable()
	if executableErr != nil {
		return writeAlfred(alfred.Notice("Kwel could not locate the Workflow Bridge."))
	}
	prepared, prepareErr := prepareBridgeForSearch(executable, home, allowedExtensionID)
	if prepareErr != nil {
		return writeAlfred(alfred.Notice("Kwel could not register its Chrome connection safely."))
	}
	if prepared {
		return writeAlfred(alfred.Notice("Kwel is connecting to Alfred. Try this search again in a moment."))
	}
	updated, updateErr := install.UpdateIfNeeded(executable, home, allowedExtensionID, version)
	if updateErr != nil {
		return writeAlfred(alfred.Notice("Kwel could not update its Chrome connection safely."))
	}
	if updated {
		return writeAlfred(alfred.Notice("Kwel is reconnecting after a Workflow update. Try this search again in a moment."))
	}

	directory, err := install.EnsureSocketDirectory()
	if err != nil {
		return writeAlfred(alfred.Notice("Kwel could not access its private Chrome connection."))
	}
	return writeAlfred(searchProfile(directory, configuredProfileID, *query, *limit))
}

func searchProfile(directory, profileID, query string, limit int) alfred.Output {
	socketPath, err := install.ProfileSocketPath(directory, profileID)
	if err != nil {
		return alfred.ProfileConfigurationRequired()
	}
	result, err := relay.Search(socketPath, profileID, query, limit)
	if err != nil {
		var clientError *relay.ClientError
		if errors.As(err, &clientError) {
			return alfred.Notice(clientError.Message)
		}
		return alfred.Notice("Kwel search is unavailable. Check Chrome and Kwel, then retry.")
	}
	return alfred.Results(result, profileID)
}

func openResult(args []string) error {
	set := flag.NewFlagSet("open", flag.ContinueOnError)
	set.SetOutput(io.Discard)
	profileID := set.String("profile-id", "", "Chrome profile used for the search")
	openRef := set.String("ref", "", "Opaque Kwel result reference")
	if err := set.Parse(args); err != nil || set.NArg() != 0 || !protocol.ValidProfileID(*profileID) || !protocol.ValidProfileID(*openRef) {
		return errors.New("invalid Kwel result")
	}
	*profileID = strings.ToLower(*profileID)
	directory, err := install.EnsureSocketDirectory()
	if err != nil {
		return errors.New("Kwel profile is disconnected")
	}
	socketPath, err := install.ProfileSocketPath(directory, *profileID)
	if err != nil {
		return errors.New("Kwel profile is invalid")
	}
	return openResultAndActivate(socketPath, *profileID, *openRef, activateChrome)
}

func openResultAndActivate(socketPath, profileID, openRef string, activate func() error) error {
	if err := relay.Open(socketPath, profileID, openRef); err != nil {
		return err
	}
	return activate()
}

func activateChrome() error {
	// The page has already been opened by Kwel in the requested profile.
	// Activate only the application; no URL or profile is sent to Launch Services.
	if err := exec.Command("/usr/bin/open", "-b", "com.google.Chrome").Run(); err != nil {
		return errors.New("Kwel opened the page, but Chrome could not be brought to the front")
	}
	return nil
}

// prepareBridgeForSearch installs or repairs the host registration on the first
// explicit Workflow search. It is never shown as a separate Alfred command.
func prepareBridgeForSearch(executable, home, extensionID string) (bool, error) {
	report := install.Doctor(home, extensionID)
	switch searchSetupActionFor(report) {
	case searchSetupInstall:
		if err := install.Setup(executable, home, extensionID); err != nil {
			return true, err
		}
		_ = install.StopActiveHost(home)
		return true, nil
	case searchSetupRepair:
		if err := install.Repair(executable, home, extensionID); err != nil {
			return true, err
		}
		_ = install.StopActiveHost(home)
		return true, nil
	}
	return false, nil
}

type searchSetupAction int

const (
	searchSetupNone searchSetupAction = iota
	searchSetupInstall
	searchSetupRepair
)

func searchSetupActionFor(report install.DoctorReport) searchSetupAction {
	if report.State == "bridge_not_installed" {
		return searchSetupInstall
	}
	if report.State == "bridge_needs_repair" && report.BridgeInstalled && !report.HostRegistered {
		return searchSetupRepair
	}
	return searchSetupNone
}

func uninstall(args []string) error {
	format, err := outputFormat(args)
	if err != nil {
		return writeOperationResult(format, false, "Kwel Bridge removal could not start.")
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return writeOperationResult(format, false, "Kwel Bridge could not read the current user profile.")
	}
	report := install.Doctor(home, allowedExtensionID)
	if report.State == "profile_conflict" {
		return writeOperationResult(format, false, "Close the duplicate Chrome connection, then retry.")
	}
	_ = install.StopActiveHost(home)
	if err := install.Uninstall(home); err != nil {
		return writeOperationResult(format, false, "Kwel Bridge could not be removed safely. No files were removed.")
	}
	for attempt := 0; attempt < 10; attempt++ {
		if len(install.ProfileSocketFiles()) == 0 {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	for _, socketPath := range install.ProfileSocketFiles() {
		if err := securefs.VerifyOwnedSocket(socketPath); err == nil {
			if connection, dialErr := net.DialTimeout("unix", socketPath, 100*time.Millisecond); dialErr != nil {
				_ = os.Remove(socketPath)
			} else {
				_ = connection.Close()
			}
		}
	}
	return writeOperationResult(format, true, "Kwel Native Messaging registration and Bridge executable were removed.")
}

func outputFormat(args []string) (string, error) {
	set := flag.NewFlagSet("output", flag.ContinueOnError)
	set.SetOutput(io.Discard)
	format := set.String("format", "text", "Output format")
	if err := set.Parse(args); err != nil || set.NArg() != 0 || (*format != "text" && *format != "alfred" && *format != "json") {
		return "", errors.New("invalid output format")
	}
	return *format, nil
}

func writeOperationResult(format string, ok bool, message string) error {
	if format == "alfred" {
		return writeAlfred(alfred.Notice(message))
	}
	if !ok {
		return errors.New(message)
	}
	fmt.Println(message)
	return nil
}

func writeAlfred(output alfred.Output) error {
	data, err := alfred.Encode(output)
	if err != nil {
		return err
	}
	_, err = os.Stdout.Write(append(data, '\n'))
	return err
}

func usageError() error {
	return errors.New("usage: kwel-bridge search|open|uninstall|version")
}

func safeError(err error) string {
	if err == nil {
		return "unknown error"
	}
	message := strings.TrimSpace(err.Error())
	if message == "" {
		return "operation failed"
	}
	return filepath.Base(message)
}
