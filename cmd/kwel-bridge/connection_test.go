package main

import (
	"net"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	"com.kwel/alfredbridge/internal/alfred"
	"com.kwel/alfredbridge/internal/protocol"
	"com.kwel/alfredbridge/internal/testutil"
)

func TestConnectionWaitDeadlineIsBoundedAndPreserved(t *testing.T) {
	now := time.Unix(1_700_000_000, 0)
	stamp := func(value time.Time) string { return strconv.FormatInt(value.UnixMilli(), 10) }
	failure := alfred.Notice("The configured profile is disconnected.")
	for _, test := range []struct {
		name     string
		previous string
		deadline time.Time
		waiting  bool
	}{
		{"first run", "", now.Add(connectionWaitWindow), true},
		{"preserve across reruns and query edits", stamp(now.Add(20 * time.Second)), now.Add(20 * time.Second), true},
		{"invalid session value", "invalid", now.Add(connectionWaitWindow), true},
		{"bound a future session value", stamp(now.Add(time.Hour)), now.Add(connectionWaitWindow), true},
		{"deadline reached", stamp(now), time.Time{}, false},
		{"deadline expired", stamp(now.Add(-time.Second)), time.Time{}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			output := waitForConnection(failure, test.previous, now)
			if !output.SkipKnowledge || len(output.Items) != 1 || output.Items[0].Valid || output.Items[0].Arg != "" {
				t.Fatalf("connection state must be a single disabled item: %#v", output)
			}
			if test.waiting {
				if output.Rerun != 1 || output.Variables[connectionDeadlineVariable] != stamp(test.deadline) {
					t.Fatalf("missing or extended connection retry: %#v", output)
				}
			} else if output.Rerun != 0 || output.Variables[connectionDeadlineVariable] != "" || output.Items[0].Title != failure.Items[0].Title {
				t.Fatalf("expired wait must stop and show the connection error: %#v", output)
			}
		})
	}
}

func TestDisconnectedProfileRefreshesUntilConfiguredHostAppears(t *testing.T) {
	t.Setenv(connectionDeadlineVariable, "")
	directory := testutil.SocketDir(t)
	profile := "00000000-0000-4000-8000-00000000000a"
	otherRequests := mockConfiguredProfile(t, directory, "00000000-0000-4000-8000-00000000000b", "Other profile")
	waiting := searchProfile(directory, profile, "Kyoto", 20)
	deadline := waiting.Variables[connectionDeadlineVariable]
	if waiting.Rerun != 1 || deadline == "" {
		t.Fatalf("an unconnected profile did not schedule an Alfred rerun: %#v", waiting)
	}
	t.Setenv(connectionDeadlineVariable, deadline)
	changed := searchProfile(directory, profile, "Kyoto travel", 20)
	if changed.Rerun != 1 || changed.Variables[connectionDeadlineVariable] != deadline {
		t.Fatalf("editing the query restarted the connection wait: %#v", changed)
	}
	select {
	case <-otherRequests:
		t.Fatal("connection wait searched a different Chrome profile")
	default:
	}
	requests := mockConfiguredProfile(t, directory, profile, "Connected result")
	result := searchProfile(directory, profile, "Kyoto travel", 20)
	if result.Rerun != 0 || result.Variables[connectionDeadlineVariable] != "" || len(result.Items) != 1 || !result.Items[0].Valid || result.Items[0].Variables["KWEL_RESULT_PROFILE_ID"] != profile {
		t.Fatalf("connection did not replace the waiting notice with bound results: %#v", result)
	}
	select {
	case request := <-requests:
		if request.Query != "Kyoto travel" || request.ProfileID != profile || request.Limit != 20 {
			t.Fatalf("rerun changed the query or target profile: %#v", request)
		}
	case <-time.After(time.Second):
		t.Fatal("query did not reach the configured profile")
	}
}

func TestConnectedHostErrorsStopConnectionWait(t *testing.T) {
	for _, code := range []string{protocol.CodeProfileConflict, protocol.CodePermissionRequired, protocol.CodeIncompatible, protocol.CodeSearchFailed, protocol.CodeTimeout} {
		t.Run(code, func(t *testing.T) {
			t.Setenv(connectionDeadlineVariable, strconv.FormatInt(time.Now().Add(connectionWaitWindow).UnixMilli(), 10))
			directory := testutil.SocketDir(t)
			profile := "00000000-0000-4000-8000-00000000000a"
			listener, err := net.Listen("unix", filepath.Join(directory, profile+".sock"))
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = listener.Close() })
			finished := make(chan error, 1)
			go func() {
				connection, err := listener.Accept()
				if err != nil {
					finished <- err
					return
				}
				defer connection.Close()
				_ = connection.SetDeadline(time.Now().Add(time.Second))
				payload, err := protocol.ReadFrame(connection)
				if err != nil {
					finished <- err
					return
				}
				request, err := protocol.Decode(payload)
				if err != nil {
					finished <- err
					return
				}
				finished <- protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeError, RequestID: request.RequestID, Code: code, Text: "Search unavailable."})
			}()
			output := searchProfile(directory, profile, "Kyoto", 20)
			select {
			case err := <-finished:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(time.Second):
				t.Fatal("host error fixture did not finish")
			}
			if output.Rerun != 0 || output.Variables[connectionDeadlineVariable] != "" || len(output.Items) != 1 || output.Items[0].Valid {
				t.Fatalf("host error was retried or made actionable: %#v", output)
			}
		})
	}
}
