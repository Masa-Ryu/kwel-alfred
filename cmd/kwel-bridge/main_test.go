package main

import (
	"encoding/json"
	"io"
	"net"
	"os"
	"path/filepath"
	"testing"
	"time"

	"com.kwel/alfredbridge/internal/alfred"
	"com.kwel/alfredbridge/internal/install"
	"com.kwel/alfredbridge/internal/protocol"
)

func TestSearchSetupActionForBridgeStates(t *testing.T) {
	tests := []struct {
		name   string
		report install.DoctorReport
		want   searchSetupAction
	}{
		{
			name:   "install missing Bridge",
			report: install.DoctorReport{State: "bridge_not_installed"},
			want:   searchSetupInstall,
		},
		{
			name:   "repair recognizable Bridge registration",
			report: install.DoctorReport{State: "bridge_needs_repair", BridgeInstalled: true},
			want:   searchSetupRepair,
		},
		{
			name:   "leave an unsafe Bridge path for manual review",
			report: install.DoctorReport{State: "bridge_needs_repair"},
			want:   searchSetupNone,
		},
		{
			name:   "leave a ready Bridge unchanged",
			report: install.DoctorReport{State: "ready", BridgeInstalled: true, HostRegistered: true},
			want:   searchSetupNone,
		},
		{
			name:   "leave profile conflicts untouched",
			report: install.DoctorReport{State: "profile_conflict", BridgeInstalled: true, HostRegistered: true},
			want:   searchSetupNone,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := searchSetupActionFor(test.report); got != test.want {
				t.Fatalf("setup action = %d; want %d", got, test.want)
			}
		})
	}
}

func TestSearchValidatesProfileConfigurationBeforeHostSetup(t *testing.T) {
	previousSupport := kwelClientSupport
	kwelClientSupport = "false"
	defer func() { kwelClientSupport = previousSupport }()
	for _, value := range []string{"", " ", "work", "mhablbmgafbdofgaffhdnnknjhafmlin", "00000000-0000-0000-0000-000000000000", "550e8400-e29b-41d4-a716-446655440000/../other"} {
		t.Run(value, func(t *testing.T) {
			output := captureSearch(t, []string{"--profile-id", value, "--query", "github actions"})
			if len(output.Items) != 1 || output.Items[0].Title != "Configure the Kwel Profile ID" || output.Items[0].Valid {
				t.Fatalf("invalid configuration reached search: %#v", output)
			}
		})
	}
	for _, value := range []string{"550e8400-e29b-41d4-a716-446655440000", " 550E8400-E29B-41D4-A716-446655440000 "} {
		output := captureSearch(t, []string{"--profile-id", value, "--query", "日本語 github actions"})
		if len(output.Items) != 1 || output.Items[0].Title != "This Kwel extension build does not support Alfred integration." {
			t.Fatalf("valid UUID configuration was rejected: %#v", output)
		}
	}
}

func TestSearchProfileChangesUseOnlyConfiguredSocketAndBindResults(t *testing.T) {
	directory, err := os.MkdirTemp("/private/tmp", "kw-profile-test-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	profileA := "550e8400-e29b-41d4-a716-446655440000"
	profileB := "550e8400-e29b-41d4-a716-446655440001"
	requestsA := mockConfiguredProfile(t, directory, profileA, "Result A")
	requestsB := mockConfiguredProfile(t, directory, profileB, "Result B")
	for _, test := range []struct {
		profileID string
		query     string
		title     string
		requests  <-chan protocol.Message
	}{
		{profileA, "github actions", "Result A", requestsA},
		{profileB, "日本語検索", "Result B", requestsB},
	} {
		output := searchProfile(directory, test.profileID, test.query, 20)
		if len(output.Items) != 1 || output.Items[0].Title != test.title || output.Items[0].Variables["KWEL_RESULT_PROFILE_ID"] != test.profileID || !output.Items[0].Valid {
			t.Fatalf("configured profile was not honored: %#v", output)
		}
		select {
		case request := <-test.requests:
			if request.ProfileID != test.profileID || request.Query != test.query || request.Limit != 20 {
				t.Fatalf("profile, Japanese query, or limit changed: %#v", request)
			}
		case <-time.After(time.Second):
			t.Fatal("request did not reach configured socket")
		}
	}
	output := searchProfile(directory, "550e8400-e29b-41d4-a716-446655440002", "query", 20)
	if len(output.Items) != 1 || output.Items[0].Valid || output.Items[0].Arg != "" {
		t.Fatalf("unconnected ID produced actionable results: %#v", output)
	}
	select {
	case <-requestsA:
		t.Fatal("missing ID fell back to A")
	case <-requestsB:
		t.Fatal("missing ID fell back to B")
	default:
	}
}

func captureSearch(t *testing.T, args []string) alfred.Output {
	t.Helper()
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	previous := os.Stdout
	os.Stdout = writer
	defer func() { os.Stdout = previous }()
	err = search(args)
	_ = writer.Close()
	os.Stdout = previous
	if err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(reader)
	_ = reader.Close()
	if err != nil {
		t.Fatal(err)
	}
	var output alfred.Output
	if err := json.Unmarshal(data, &output); err != nil {
		t.Fatalf("invalid Alfred output: %v", err)
	}
	return output
}

func mockConfiguredProfile(t *testing.T, directory, profileID, title string) <-chan protocol.Message {
	t.Helper()
	listener, err := net.Listen("unix", filepath.Join(directory, profileID+".sock"))
	if err != nil {
		t.Fatal(err)
	}
	requests := make(chan protocol.Message, 4)
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		for {
			connection, err := listener.Accept()
			if err != nil {
				return
			}
			_ = connection.SetDeadline(time.Now().Add(time.Second))
			payload, err := protocol.ReadFrame(connection)
			if err == nil {
				request, decodeErr := protocol.Decode(payload)
				if decodeErr == nil {
					requests <- request
					hasMore := false
					_ = protocol.WriteFrame(connection, protocol.Message{V: 1, Type: protocol.TypeSearchResult, RequestID: request.RequestID, HasMore: &hasMore, Items: []protocol.Item{{Title: title, URL: "https://example.com/", OpenRef: "111e8400-e29b-41d4-a716-446655440001"}}})
				}
			}
			_ = connection.Close()
		}
	}()
	t.Cleanup(func() {
		_ = listener.Close()
		select {
		case <-finished:
		case <-time.After(2 * time.Second):
			t.Error("mock configured profile did not stop")
		}
	})
	return requests
}

func TestOpenRequiresResultProfileAndReferenceWithoutConfigurationFallback(t *testing.T) {
	t.Setenv("KWEL_PROFILE_ID", "550e8400-e29b-41d4-a716-446655440001")
	for _, args := range [][]string{
		{"--ref", "111e8400-e29b-41d4-a716-446655440001"},
		{"--profile-id", "550e8400-e29b-41d4-a716-446655440000"},
		{"--profile-id", "550e8400-e29b-41d4-a716-446655440000", "--ref", "https://example.com/"},
	} {
		if err := openResult(args); err == nil || err.Error() != "invalid Kwel result" {
			t.Fatalf("invalid result binding was not rejected before connection: %v", err)
		}
	}
}
