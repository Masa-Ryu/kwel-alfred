package relay

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
)

func TestMissingConfiguredProfileDoesNotUseConnectedProfiles(t *testing.T) {
	directory := shortSocketDir(t)
	var outputA, outputB bytes.Buffer
	serveRoutingProfile(t, directory, newTestSession(&outputA, profileA))
	serveRoutingProfile(t, directory, newTestSession(&outputB, profileB))
	missingID := "00000000-0000-4000-8000-00000000000c"
	missingSocket := filepath.Join(directory, missingID+".sock")

	_, err := Search(missingSocket, missingID, "Café Kyoto", 20)
	requireRoutingError(t, err, protocol.CodeDisconnected)
	if err.Error() != "The configured Kwel profile is not connected. Check Alfred Integration in that Chrome profile." {
		t.Fatalf("missing connection was reported as an in-flight disconnect: %v", err)
	}
	err = Open(missingSocket, missingID, openRefA)
	requireRoutingError(t, err, protocol.CodeDisconnected)
	if outputA.Len() != 0 || outputB.Len() != 0 {
		t.Fatal("a missing configured profile was forwarded to another connected profile")
	}
}

func TestWrongProfileSocketRejectsSearchAndOpenBeforeNativeMessaging(t *testing.T) {
	directory := shortSocketDir(t)
	var nativeOutput bytes.Buffer
	path := serveRoutingProfile(t, directory, newTestSession(&nativeOutput, profileA))

	_, err := Search(path, profileB, "Kyoto", 20)
	requireRoutingError(t, err, protocol.CodeInvalidRequest)
	err = Open(path, profileB, openRefA)
	requireRoutingError(t, err, protocol.CodeInvalidRequest)
	if nativeOutput.Len() != 0 {
		t.Fatal("a request for another profile reached this profile's extension")
	}
}

func TestNativeHelloAckWriteFailureReleasesProfileSocket(t *testing.T) {
	directory := shortSocketDir(t)
	var input bytes.Buffer
	if err := protocol.WriteFrame(&input, protocol.Message{V: 1, Type: protocol.TypeHello, ProfileID: profileA}); err != nil {
		t.Fatal(err)
	}
	err := RunNativeHost(&input, routingFailedWriter{}, "chrome-extension://"+testExtensionID+"/", testExtensionID, "test", directory)
	if err == nil {
		t.Fatal("Native Host accepted a failed hello acknowledgement write")
	}
	path := filepath.Join(directory, profileA+".sock")
	waitFor(t, func() bool {
		_, err := os.Lstat(path)
		return errors.Is(err, os.ErrNotExist)
	})
}

func TestDisconnectedConfiguredProfileDoesNotUseRemainingConnection(t *testing.T) {
	directory := shortSocketDir(t)
	var outputA, outputB bytes.Buffer
	sessionA := newTestSession(&outputA, profileA)
	pathA := serveRoutingProfile(t, directory, sessionA)
	serveRoutingProfile(t, directory, newTestSession(&outputB, profileB))
	sessionA.close()
	waitFor(t, func() bool {
		_, err := os.Lstat(pathA)
		return errors.Is(err, os.ErrNotExist)
	})

	_, err := Search(pathA, profileA, "Kyoto", 20)
	requireRoutingError(t, err, protocol.CodeDisconnected)
	err = Open(pathA, profileA, openRefA)
	requireRoutingError(t, err, protocol.CodeDisconnected)
	if outputA.Len() != 0 || outputB.Len() != 0 {
		t.Fatal("a disconnected profile's request was sent to a remaining connection")
	}
}

func TestLateCanceledSearchResponseCannotSatisfyNextSearch(t *testing.T) {
	session := newTestSession(io.Discard, profileA)
	defer session.close()
	oldContext, cancelOld := context.WithCancel(context.Background())
	defer cancelOld()
	oldResult := make(chan protocol.Message, 1)
	go func() {
		oldResult <- session.Request(oldContext, protocol.Message{V: 1, Type: protocol.TypeSearch, ProfileID: profileA, RequestID: "old-query", Query: "old", Limit: 20})
	}()
	waitForRoutingRequest(t, session, "old-query")
	cancelOld()
	select {
	case result := <-oldResult:
		if result.Code != protocol.CodeDisconnected {
			t.Fatalf("canceled request response = %#v", result)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("canceled search did not finish")
	}

	newContext, cancelNew := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancelNew()
	newResult := make(chan protocol.Message, 1)
	go func() {
		newResult <- session.Request(newContext, protocol.Message{V: 1, Type: protocol.TypeSearch, ProfileID: profileA, RequestID: "new-query", Query: "new", Limit: 20})
	}()
	waitForRoutingRequest(t, session, "new-query")
	session.deliver(routingSearchResponse("old-query", "Old response", openRefA))
	session.deliver(routingSearchResponse("new-query", "New response", openRefA))
	select {
	case result := <-newResult:
		if result.Type != protocol.TypeSearchResult || result.RequestID != "new-query" || len(result.Items) != 1 || result.Items[0].Title != "New response" {
			t.Fatalf("old response entered the new search: %#v", result)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("new search did not receive its own response")
	}
}

func TestExpiredOpenReferenceDoesNotFallBackToAnotherProfile(t *testing.T) {
	directory := shortSocketDir(t)
	reader, writer := io.Pipe()
	session := newTestSession(writer, profileA)
	path := serveRoutingProfile(t, directory, session)
	var otherOutput bytes.Buffer
	serveRoutingProfile(t, directory, newTestSession(&otherOutput, profileB))
	t.Cleanup(func() { _ = reader.Close() })

	finished := make(chan error, 1)
	go func() { finished <- Open(path, profileA, openRefA) }()
	request := readRoutingNativeMessage(t, reader)
	if request.Type != protocol.TypeOpenPage || request.OpenRef != openRefA {
		t.Fatalf("open request lost the original reference: %#v", request)
	}
	session.deliver(localError(request.RequestID, protocol.CodeResultExpired, "The result expired."))
	select {
	case err := <-finished:
		requireRoutingError(t, err, protocol.CodeResultExpired)
	case <-time.After(2 * time.Second):
		t.Fatal("expired open result did not finish")
	}
	if otherOutput.Len() != 0 {
		t.Fatal("expired result was forwarded to a different profile")
	}
}

func TestSearchAndOpenStayBoundToEachConnectedProfile(t *testing.T) {
	directory := shortSocketDir(t)
	type connectedProfile struct {
		id      string
		ref     string
		path    string
		session *Session
		reader  *io.PipeReader
	}
	profiles := make([]connectedProfile, 0, 2)
	for index, id := range []string{profileA, profileB} {
		reader, writer := io.Pipe()
		session := newTestSession(writer, id)
		path := serveRoutingProfile(t, directory, session)
		ref := openRefA
		if index == 1 {
			ref = "00000000-0000-4000-8000-000000000102"
		}
		profiles = append(profiles, connectedProfile{id: id, ref: ref, path: path, session: session, reader: reader})
		t.Cleanup(func() { _ = reader.Close() })
	}

	for _, profile := range profiles {
		profile := profile
		t.Run(profile.id, func(t *testing.T) {
			searchFinished := make(chan error, 1)
			go func() {
				result, err := Search(profile.path, profile.id, "Café Kyoto", 20)
				if err == nil && (len(result.Items) != 1 || result.Items[0].OpenRef != profile.ref) {
					err = errors.New("search returned a reference from the wrong profile")
				}
				searchFinished <- err
			}()
			request := readRoutingNativeMessage(t, profile.reader)
			if request.Type != protocol.TypeSearch || request.Query != "Café Kyoto" || request.Limit != 20 {
				t.Fatalf("wrong search reached profile: %#v", request)
			}
			profile.session.deliver(routingSearchResponse(request.RequestID, "Result for "+profile.id, profile.ref))
			select {
			case err := <-searchFinished:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("search did not finish on its original profile")
			}

			openFinished := make(chan error, 1)
			go func() { openFinished <- Open(profile.path, profile.id, profile.ref) }()
			openRequest := readRoutingNativeMessage(t, profile.reader)
			if openRequest.Type != protocol.TypeOpenPage || openRequest.OpenRef != profile.ref || openRequest.ProfileID != "" {
				t.Fatalf("wrong open request reached profile: %#v", openRequest)
			}
			profile.session.deliver(protocol.Message{V: 1, Type: protocol.TypeOpenResult, RequestID: openRequest.RequestID, OK: true})
			select {
			case err := <-openFinished:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("open did not finish on its original profile")
			}
		})
	}
}

func serveRoutingProfile(t *testing.T, directory string, session *Session) string {
	t.Helper()
	path := filepath.Join(directory, session.profileID+".sock")
	ready := make(chan error, 1)
	finished := make(chan error, 1)
	go func() { finished <- (SocketServer{Path: path, Session: session}).Serve(session.closed, ready) }()
	t.Cleanup(func() {
		session.close()
		if closer, ok := session.out.(io.Closer); ok {
			_ = closer.Close()
		}
		select {
		case <-finished:
		case <-time.After(2 * time.Second):
			t.Error("profile socket server did not stop")
		}
	})
	select {
	case err := <-ready:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("profile socket server did not start")
	}
	return path
}

func requireRoutingError(t *testing.T, err error, code string) {
	t.Helper()
	var clientError *ClientError
	if !errors.As(err, &clientError) || clientError.Code != code {
		t.Fatalf("error = %v, expected %s", err, code)
	}
}

func waitForRoutingRequest(t *testing.T, session *Session, requestID string) {
	t.Helper()
	waitFor(t, func() bool {
		session.mu.Lock()
		defer session.mu.Unlock()
		return session.pending[requestID] != nil
	})
}

func routingSearchResponse(requestID, title, openRef string) protocol.Message {
	hasMore := false
	return protocol.Message{V: 1, Type: protocol.TypeSearchResult, RequestID: requestID, HasMore: &hasMore, Items: []protocol.Item{{Title: title, URL: "https://example.com/", OpenRef: openRef}}}
}

func readRoutingNativeMessage(t *testing.T, reader io.Reader) protocol.Message {
	t.Helper()
	type readResult struct {
		message protocol.Message
		err     error
	}
	finished := make(chan readResult, 1)
	go func() {
		payload, err := protocol.ReadFrame(reader)
		var message protocol.Message
		if err == nil {
			message, err = protocol.Decode(payload)
		}
		finished <- readResult{message: message, err: err}
	}()
	select {
	case result := <-finished:
		if result.err != nil {
			t.Fatal(result.err)
		}
		return result.message
	case <-time.After(2 * time.Second):
		t.Fatal("request was not sent to the expected profile")
		return protocol.Message{}
	}
}

type routingFailedWriter struct{}

func (routingFailedWriter) Write([]byte) (int, error) {
	return 0, errors.New("native output closed")
}
