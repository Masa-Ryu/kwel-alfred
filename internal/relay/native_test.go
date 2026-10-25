package relay

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
)

const (
	testExtensionID = "mhablbmgafbdofgaffhdnnknjhafmlin"
	profileA        = "550e8400-e29b-41d4-a716-446655440000"
	profileB        = "550e8400-e29b-41d4-a716-446655440001"
	openRefA        = "111e8400-e29b-41d4-a716-446655440001"
)

func TestValidNativeOrigin(t *testing.T) {
	for _, origin := range []string{"chrome-extension://" + testExtensionID, "chrome-extension://" + testExtensionID + "/"} {
		if !ValidNativeOrigin(origin, testExtensionID) {
			t.Errorf("valid origin rejected: %s", origin)
		}
	}
	for _, origin := range []string{
		"chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
		"https://" + testExtensionID + "/",
		"chrome-extension://" + testExtensionID + "/path",
		"chrome-extension://" + testExtensionID + "/?query=1",
	} {
		if ValidNativeOrigin(origin, testExtensionID) {
			t.Errorf("invalid origin accepted: %s", origin)
		}
	}
}

func TestNativeHostRejectsSpoofedOriginAndInvalidHello(t *testing.T) {
	directory := shortSocketDir(t)
	var output bytes.Buffer
	if err := RunNativeHost(nil, &output, "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/", testExtensionID, "test", directory); err == nil {
		t.Fatal("spoofed Chrome caller origin was accepted")
	}
	if output.Len() != 0 {
		t.Fatal("rejected caller wrote to Native Messaging stdout")
	}

	var input bytes.Buffer
	if err := protocol.WriteFrame(&input, protocol.Message{V: protocol.Version, Type: protocol.TypeHello, ProfileID: "invalid", ProfileLabel: "Work"}); err != nil {
		t.Fatal(err)
	}
	output.Reset()
	if err := RunNativeHost(&input, &output, "chrome-extension://"+testExtensionID+"/", testExtensionID, "test", directory); err == nil {
		t.Fatal("invalid Native Messaging hello was accepted")
	}
	if output.Len() != 0 {
		t.Fatal("invalid hello produced a protocol response not defined by Kwel")
	}
}

func TestSessionCorrelatesConcurrentResponses(t *testing.T) {
	reader, writer := io.Pipe()
	session := newTestSession(writer, profileA)
	defer func() {
		session.close()
		_ = writer.Close()
		_ = reader.Close()
	}()
	requests := []protocol.Message{
		{V: 1, Type: protocol.TypeSearch, RequestID: "one", Query: "日本語", Limit: 20},
		{V: 1, Type: protocol.TypeSearch, RequestID: "two", Query: "space and symbols !", Limit: 20},
	}
	results := make(chan protocol.Message, 2)
	var group sync.WaitGroup
	for _, request := range requests {
		group.Add(1)
		go func(request protocol.Message) {
			defer group.Done()
			results <- session.Request(context.Background(), request)
		}(request)
	}
	outgoing := make([]protocol.Message, 0, 2)
	for len(outgoing) < 2 {
		message := readMessage(t, reader)
		outgoing = append(outgoing, message)
	}
	for index := len(outgoing) - 1; index >= 0; index-- {
		message := outgoing[index]
		hasMore := false
		ref := openRefA
		if message.RequestID == "one" {
			ref = "222e8400-e29b-41d4-a716-446655440002"
		}
		session.deliver(protocol.Message{V: 1, Type: protocol.TypeSearchResult, RequestID: message.RequestID, HasMore: &hasMore, Items: []protocol.Item{{Title: message.RequestID, URL: "https://example.com/" + message.RequestID, OpenRef: ref}}})
	}
	group.Wait()
	close(results)
	seen := map[string]bool{}
	for result := range results {
		if result.Type != protocol.TypeSearchResult || result.RequestID == "" || result.Items[0].Title != result.RequestID {
			t.Fatalf("response correlation failed: %#v", result)
		}
		seen[result.RequestID] = true
	}
	if len(seen) != 2 {
		t.Fatalf("received %d correlated results", len(seen))
	}
}

func TestSessionTimeoutAndProfileConflict(t *testing.T) {
	session := newTestSession(io.Discard, profileA)
	defer session.close()
	request := protocol.Message{V: 1, Type: protocol.TypeSearch, RequestID: "timeout", Query: "x", Limit: 20}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Millisecond)
	defer cancel()
	response := session.Request(ctx, request)
	if response.Code != protocol.CodeTimeout {
		t.Fatalf("timeout code = %q", response.Code)
	}
	if err := protocol.ValidateSearchResult(response, "timeout"); err != nil {
		t.Fatalf("valid local timeout rejected: %v", err)
	}
	session.addClaim("duplicate-session")
	response = session.Request(context.Background(), protocol.Message{V: 1, Type: protocol.TypeSearch, RequestID: "conflict", Query: "x", Limit: 20})
	if response.Code != protocol.CodeProfileConflict {
		t.Fatalf("conflict code = %q", response.Code)
	}
	if err := protocol.ValidateSearchResult(response, "conflict"); err != nil {
		t.Fatalf("valid local profile conflict rejected: %v", err)
	}
}

func TestSocketConflictFailsClosedUntilDuplicateReleasesClaim(t *testing.T) {
	directory := shortSocketDir(t)
	socketPath := filepath.Join(directory, profileA+".sock")
	owner := newTestSession(io.Discard, profileA)
	ownerDone := make(chan struct{})
	ownerErrors := make(chan error, 1)
	go func() { ownerErrors <- (SocketServer{Path: socketPath, Session: owner}).Serve(ownerDone) }()
	waitForSocketOrError(t, socketPath, ownerErrors)

	duplicate := newTestSession(io.Discard, profileA)
	duplicateDone := make(chan struct{})
	duplicateReady := make(chan error, 1)
	duplicateDoneResult := make(chan error, 1)
	go func() {
		duplicateDoneResult <- (SocketServer{Path: socketPath, Session: duplicate}).Serve(duplicateDone, duplicateReady)
	}()
	select {
	case err := <-duplicateReady:
		if !errors.Is(err, ErrProfileConflict) {
			t.Fatalf("duplicate profile error = %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("duplicate profile was not rejected")
	}
	if !owner.isConflicted() {
		t.Fatal("existing profile owner did not fail closed after a duplicate claim")
	}
	message, err := Control(socketPath, protocol.Message{V: protocol.Version, Type: protocol.TypeProfileFree, SessionID: duplicate.ID()})
	if err != nil || message.Type != protocol.TypeProfileAck || owner.isConflicted() {
		t.Fatalf("duplicate claim was not released: %#v, %v", message, err)
	}
	close(duplicateDone)
	close(ownerDone)
	owner.close()
	duplicate.close()
	select {
	case <-duplicateDoneResult:
	case <-time.After(2 * time.Second):
		t.Fatal("duplicate socket server did not stop")
	}
	waitFor(t, func() bool {
		_, err := os.Lstat(socketPath)
		return errors.Is(err, os.ErrNotExist)
	})
}

func TestTwoProfilesUseIndependentSocketsAndKeepTheirSearchResultsSeparate(t *testing.T) {
	directory := shortSocketDir(t)
	type runningProfile struct {
		id      string
		session *Session
		reader  *io.PipeReader
		path    string
		done    chan struct{}
	}
	profiles := make([]runningProfile, 0, 2)
	for _, id := range []string{profileA, profileB} {
		extensionReader, bridgeWriter := io.Pipe()
		session := newTestSession(bridgeWriter, id)
		path := filepath.Join(directory, id+".sock")
		done := make(chan struct{})
		serverErrors := make(chan error, 1)
		go func() { serverErrors <- (SocketServer{Path: path, Session: session}).Serve(done) }()
		waitForSocketOrError(t, path, serverErrors)
		profiles = append(profiles, runningProfile{id: id, session: session, reader: extensionReader, path: path, done: done})
	}
	t.Cleanup(func() {
		for _, profile := range profiles {
			close(profile.done)
			profile.session.close()
			_ = profile.reader.Close()
		}
	})

	type result struct {
		id      string
		message protocol.Message
		err     error
	}
	results := make(chan result, 2)
	for _, profile := range profiles {
		profile := profile
		go func() {
			message, err := Search(profile.path, profile.id, "query-"+profile.id[len(profile.id)-1:], 20)
			results <- result{id: profile.id, message: message, err: err}
		}()
	}

	for _, profile := range profiles {
		request := readMessage(t, profile.reader)
		wantQuery := "query-" + profile.id[len(profile.id)-1:]
		if request.Type != protocol.TypeSearch || request.ProfileID != "" || request.Query != wantQuery {
			t.Fatalf("profile %s received a different profile's request: %#v", profile.id, request)
		}
		hasMore := false
		profile.session.deliver(protocol.Message{V: 1, Type: protocol.TypeSearchResult, RequestID: request.RequestID, HasMore: &hasMore, Items: []protocol.Item{{Title: "result-" + profile.id, URL: "https://example.com/" + profile.id, OpenRef: openRefA}}})
	}
	seen := map[string]string{}
	for range profiles {
		got := <-results
		if got.err != nil || len(got.message.Items) != 1 {
			t.Fatalf("profile %s result = %#v, %v", got.id, got.message, got.err)
		}
		seen[got.id] = got.message.Items[0].Title
	}
	for _, profile := range profiles {
		if seen[profile.id] != "result-"+profile.id {
			t.Fatalf("profile %s received mismatched results: %#v", profile.id, seen)
		}
	}
}

func TestNativeHandshakeUsesProfileSocketForSearchAndOpen(t *testing.T) {
	directory := shortSocketDir(t)
	socketPath := filepath.Join(directory, profileA+".sock")
	hostInput, extensionToHost := io.Pipe()
	extensionOutput, hostToExtension := io.Pipe()
	finished := make(chan error, 1)
	go func() {
		finished <- RunNativeHost(hostInput, hostToExtension, "chrome-extension://"+testExtensionID+"/", testExtensionID, "test-version", directory)
	}()
	if err := protocol.WriteFrame(extensionToHost, protocol.Message{V: 1, Type: protocol.TypeHello, ProfileID: profileA, ProfileLabel: "Work"}); err != nil {
		t.Fatal(err)
	}
	waitForSocketOrError(t, socketPath, finished)
	ack := readMessage(t, extensionOutput)
	if ack.Type != protocol.TypeHelloAck {
		t.Fatalf("hello response = %#v", ack)
	}

	probeResult := make(chan error, 1)
	go func() {
		profile, err := ProbeProfile(socketPath, profileA)
		if err == nil && (profile.ID != profileA || profile.Label != "Work") {
			err = errors.New("profile probe returned the wrong profile")
		}
		probeResult <- err
	}()
	if message := readMessage(t, extensionOutput); message.Type != protocol.TypePing {
		t.Fatalf("connection probe message = %#v", message)
	}
	if err := protocol.WriteFrame(extensionToHost, protocol.Message{V: 1, Type: protocol.TypePong}); err != nil {
		t.Fatal(err)
	}
	if err := <-probeResult; err != nil {
		t.Fatal(err)
	}

	searchResult := make(chan struct {
		message protocol.Message
		err     error
	}, 1)
	go func() {
		message, err := Search(socketPath, profileA, "日本語 query !", 100)
		searchResult <- struct {
			message protocol.Message
			err     error
		}{message: message, err: err}
	}()
	request := readMessage(t, extensionOutput)
	if request.Type != protocol.TypeSearch || request.Query != "日本語 query !" || request.Limit != 100 {
		t.Fatalf("forwarded request mismatch: %#v", request)
	}
	hasMore := false
	if err := protocol.WriteFrame(extensionToHost, protocol.Message{V: 1, Type: protocol.TypeSearchResult, RequestID: request.RequestID, HasMore: &hasMore, Items: []protocol.Item{{Title: "Kwel result", URL: "https://example.com/", Domain: "example.com", OpenRef: openRefA}}}); err != nil {
		t.Fatal(err)
	}
	select {
	case result := <-searchResult:
		if result.err != nil || result.message.Items[0].Title != "Kwel result" || result.message.Items[0].OpenRef != openRefA {
			t.Fatalf("local search response = %#v, %v", result.message, result.err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("local search did not receive the correlated response")
	}

	openResult := make(chan error, 1)
	go func() { openResult <- Open(socketPath, profileA, openRefA) }()
	openRequest := readMessage(t, extensionOutput)
	if openRequest.Type != protocol.TypeOpenPage || openRequest.OpenRef != openRefA || openRequest.ProfileID != "" {
		t.Fatalf("open-page request leaked URL/profile or lost ref: %#v", openRequest)
	}
	if err := protocol.WriteFrame(extensionToHost, protocol.Message{V: 1, Type: protocol.TypeOpenResult, RequestID: openRequest.RequestID, OK: true}); err != nil {
		t.Fatal(err)
	}
	if err := <-openResult; err != nil {
		t.Fatal(err)
	}

	_ = extensionToHost.Close()
	_ = extensionOutput.Close()
	select {
	case err := <-finished:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Native Messaging host did not stop after Chrome disconnected")
	}
	waitFor(t, func() bool {
		_, err := os.Lstat(socketPath)
		return errors.Is(err, os.ErrNotExist)
	})
}

func newTestSession(output io.Writer, profileID string) *Session {
	return &Session{
		id:           newSessionID(),
		profileID:    profileID,
		profileLabel: "Test Profile",
		version:      "test-version",
		out:          output,
		pending:      make(map[string]chan protocol.Message),
		claims:       make(map[string]struct{}),
		closed:       make(chan struct{}),
		stop:         make(chan struct{}),
	}
}

func waitForSocketOrError(t *testing.T, path string, errorsChannel <-chan error) {
	t.Helper()
	deadline := time.After(2 * time.Second)
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case err := <-errorsChannel:
			t.Fatalf("socket server stopped before listening: %v", err)
		case <-deadline:
			t.Fatal("socket server did not listen before timeout")
		case <-ticker.C:
			info, err := os.Lstat(path)
			if err == nil && info.Mode()&os.ModeSocket != 0 {
				return
			}
		}
	}
}

func waitFor(t *testing.T, condition func() bool) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if condition() {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("condition did not become true before timeout")
}

func readMessage(t *testing.T, reader io.Reader) protocol.Message {
	t.Helper()
	payload, err := protocol.ReadFrame(reader)
	if err != nil {
		t.Fatal(err)
	}
	message, err := protocol.Decode(payload)
	if err != nil {
		t.Fatal(err)
	}
	return message
}

func shortSocketDir(t *testing.T) string {
	t.Helper()
	directory, err := os.MkdirTemp("/private/tmp", "kwa-test-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	if err := os.Chmod(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	return directory
}
