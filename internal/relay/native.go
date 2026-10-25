package relay

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"net/url"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
	"com.kwel/alfredbridge/internal/securefs"
)

const (
	SearchTimeout = 12 * time.Second
	ProbeTimeout  = 2 * time.Second
)

var extensionIDPattern = regexp.MustCompile(`^[a-p]{32}$`)

type Session struct {
	id           string
	profileID    string
	profileLabel string
	version      string
	out          io.Writer
	writeMu      sync.Mutex
	mu           sync.Mutex
	pending      map[string]chan protocol.Message
	pendingPing  chan struct{}
	pingMu       sync.Mutex
	claims       map[string]struct{}
	closed       chan struct{}
	stop         chan struct{}
	closedOnce   sync.Once
	stopOnce     sync.Once
}

type Profile struct {
	ID    string
	Label string
}

func ValidNativeOrigin(origin, expectedExtensionID string) bool {
	if !extensionIDPattern.MatchString(expectedExtensionID) {
		return false
	}
	parsed, err := url.Parse(origin)
	if err != nil || parsed.Scheme != "chrome-extension" || parsed.Host != expectedExtensionID || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return false
	}
	return parsed.Path == "" || parsed.Path == "/"
}

// RunNativeHost binds one Chrome Native Messaging connection to one profile socket.
func RunNativeHost(input io.Reader, output io.Writer, origin, extensionID, bridgeVersion, socketDirectory string) error {
	if !ValidNativeOrigin(origin, extensionID) {
		return errors.New("native messaging caller origin is not allowed")
	}
	if err := securefs.VerifyPrivateDir(socketDirectory); err != nil {
		return fmt.Errorf("verify private socket directory: %w", err)
	}
	first, err := protocol.ReadFrame(input)
	if err != nil {
		return fmt.Errorf("read native hello: %w", err)
	}
	hello, err := protocol.Decode(first)
	if err != nil {
		return errors.New("native messaging hello is invalid")
	}
	if err := protocol.ValidateProfileHello(hello); err != nil {
		return errors.New("native messaging hello is invalid")
	}
	profileID := strings.ToLower(hello.ProfileID)
	session := &Session{
		id:           newSessionID(),
		profileID:    profileID,
		profileLabel: hello.ProfileLabel,
		version:      bridgeVersion,
		out:          output,
		pending:      make(map[string]chan protocol.Message),
		claims:       make(map[string]struct{}),
		closed:       make(chan struct{}),
		stop:         make(chan struct{}),
	}

	defer session.close()

	server := SocketServer{Path: filepath.Join(socketDirectory, profileID+".sock"), Session: session}
	serverReady := make(chan error, 1)
	serverDone := make(chan error, 1)
	go func() { serverDone <- server.Serve(session.closed, serverReady) }()
	select {
	case serverErr := <-serverReady:
		if serverErr != nil {
			if errors.Is(serverErr, ErrProfileConflict) {
				profileSocket := filepath.Join(socketDirectory, profileID+".sock")
				waitForConflictingHostClose(input, profileSocket, session.ID())
			}
			return fmt.Errorf("register profile socket: %w", serverErr)
		}
	case serverErr := <-serverDone:
		if serverErr != nil {
			return fmt.Errorf("register profile socket: %w", serverErr)
		}
		return errors.New("profile socket stopped before Native Messaging handshake")
	}
	if err := session.write(protocol.Message{V: protocol.Version, Type: protocol.TypeHelloAck}); err != nil {
		return fmt.Errorf("write native hello acknowledgement: %w", err)
	}
	incoming := make(chan nativeRead, 1)
	go readNativeMessages(input, incoming, session.closed)
	for {
		select {
		case <-session.stop:
			return nil
		case serverErr := <-serverDone:
			if serverErr != nil {
				return fmt.Errorf("local profile socket stopped: %w", serverErr)
			}
			return nil
		case item := <-incoming:
			if item.err != nil {
				if errors.Is(item.err, io.EOF) || errors.Is(item.err, io.ErrUnexpectedEOF) {
					return nil
				}
				return fmt.Errorf("read native message: %w", item.err)
			}
			message, decodeErr := protocol.Decode(item.payload)
			if decodeErr != nil || protocol.ValidateEnvelope(message) != nil {
				return errors.New("native messaging JSON is invalid")
			}
			switch message.Type {
			case protocol.TypeSearchResult, protocol.TypeOpenResult, protocol.TypeError:
				session.deliver(message)
			case protocol.TypePong:
				session.deliverPing()
			default:
				return fmt.Errorf("unexpected Native Messaging response type %q", message.Type)
			}
		}
	}
}

// The extension waits up to five seconds for hello-ack. Keep the owning
// profile marked conflicted for that handshake window, then release the claim
// when the rejected Native Messaging connection closes.
func waitForConflictingHostClose(input io.Reader, socketPath, sessionID string) {
	closed := make(chan struct{})
	go func() {
		_, _ = protocol.ReadFrame(input)
		close(closed)
	}()
	timer := time.NewTimer(5 * time.Second)
	defer timer.Stop()
	select {
	case <-closed:
	case <-timer.C:
	}
	connection, err := net.DialTimeout("unix", socketPath, 250*time.Millisecond)
	if err != nil {
		return
	}
	defer connection.Close()
	_ = connection.SetDeadline(time.Now().Add(500 * time.Millisecond))
	request := protocol.Message{V: protocol.Version, Type: protocol.TypeProfileFree, SessionID: sessionID}
	if protocol.WriteFrame(connection, request) != nil {
		return
	}
	payload, err := protocol.ReadFrame(connection)
	if err != nil {
		return
	}
	response, err := protocol.Decode(payload)
	if err != nil || response.Type != protocol.TypeProfileAck || response.SessionID != sessionID {
		return
	}
}

type nativeRead struct {
	payload []byte
	err     error
}

func readNativeMessages(input io.Reader, output chan<- nativeRead, done <-chan struct{}) {
	for {
		payload, err := protocol.ReadFrame(input)
		select {
		case output <- nativeRead{payload: payload, err: err}:
		case <-done:
			return
		}
		if err != nil {
			return
		}
	}
}

func newSessionID() string {
	var value [16]byte
	if _, err := rand.Read(value[:]); err != nil {
		return fmt.Sprintf("fallback-%d", time.Now().UnixNano())
	}
	return hex.EncodeToString(value[:])
}

func (s *Session) Request(ctx context.Context, request protocol.Message) protocol.Message {
	if err := protocol.ValidateSearch(request); err != nil {
		return localError(request.RequestID, protocol.CodeInvalidRequest, "The search request is invalid.")
	}
	response, finish, conflict := s.register(request.RequestID)
	if conflict {
		return localError(request.RequestID, protocol.CodeProfileConflict, "This Chrome profile is connected more than once. Search stopped to avoid mixing profiles.")
	}
	if response == nil {
		return localError(request.RequestID, protocol.CodeInvalidRequest, "The request identifier is already in use.")
	}
	defer finish()

	nativeRequest := protocol.Message{V: protocol.Version, Type: protocol.TypeSearch, RequestID: request.RequestID, Query: request.Query, Limit: request.Limit}
	if err := s.write(nativeRequest); err != nil {
		return localError(request.RequestID, protocol.CodeDisconnected, "Kwel disconnected before the search could start.")
	}
	select {
	case result := <-response:
		if err := protocol.ValidateSearchResult(result, request.RequestID); err != nil {
			return localError(request.RequestID, protocol.CodeSearchFailed, "Kwel returned an invalid search response.")
		}
		return result
	case <-ctx.Done():
		code, text := protocol.CodeTimeout, "Kwel search took too long. Try again."
		if errors.Is(ctx.Err(), context.Canceled) {
			code, text = protocol.CodeDisconnected, "Kwel disconnected before the search finished."
		}
		return localError(request.RequestID, code, text)
	case <-s.closed:
		return localError(request.RequestID, protocol.CodeDisconnected, "Kwel disconnected before the search finished.")
	}
}

func (s *Session) Open(ctx context.Context, request protocol.Message) protocol.Message {
	if err := protocol.ValidateOpenPage(request); err != nil || request.ProfileID != s.profileID {
		return localError(request.RequestID, protocol.CodeInvalidRequest, "The open request is invalid.")
	}
	response, finish, conflict := s.register(request.RequestID)
	if conflict {
		return localError(request.RequestID, protocol.CodeProfileConflict, "This Chrome profile is connected more than once. Search stopped to avoid mixing profiles.")
	}
	if response == nil {
		return localError(request.RequestID, protocol.CodeInvalidRequest, "The request identifier is already in use.")
	}
	defer finish()

	nativeRequest := protocol.Message{V: protocol.Version, Type: protocol.TypeOpenPage, RequestID: request.RequestID, OpenRef: request.OpenRef}
	if err := s.write(nativeRequest); err != nil {
		return localError(request.RequestID, protocol.CodeDisconnected, "Kwel disconnected before the page could be opened.")
	}
	select {
	case result := <-response:
		if err := protocol.ValidateOpenResult(result, request.RequestID); err != nil {
			return localError(request.RequestID, protocol.CodeOpenFailed, "Kwel returned an invalid open response.")
		}
		return result
	case <-ctx.Done():
		return localError(request.RequestID, protocol.CodeTimeout, "Kwel took too long to open the page.")
	case <-s.closed:
		return localError(request.RequestID, protocol.CodeDisconnected, "Kwel disconnected before the page could be opened.")
	}
}

func (s *Session) Probe(ctx context.Context) error {
	if s.isConflicted() {
		return &ClientError{Code: protocol.CodeProfileConflict, Message: "This Chrome profile is connected more than once."}
	}
	s.pingMu.Lock()
	defer s.pingMu.Unlock()
	response := make(chan struct{}, 1)
	s.mu.Lock()
	s.pendingPing = response
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		if s.pendingPing == response {
			s.pendingPing = nil
		}
		s.mu.Unlock()
	}()
	if err := s.write(protocol.Message{V: protocol.Version, Type: protocol.TypePing}); err != nil {
		return &ClientError{Code: protocol.CodeDisconnected, Message: "Kwel profile disconnected."}
	}
	select {
	case <-response:
		if s.isConflicted() {
			return &ClientError{Code: protocol.CodeProfileConflict, Message: "This Chrome profile is connected more than once."}
		}
		return nil
	case <-ctx.Done():
		return &ClientError{Code: protocol.CodeDisconnected, Message: "Kwel profile did not respond to a connection check."}
	case <-s.closed:
		return &ClientError{Code: protocol.CodeDisconnected, Message: "Kwel profile disconnected."}
	}
}

func (s *Session) register(requestID string) (chan protocol.Message, func(), bool) {
	response := make(chan protocol.Message, 1)
	s.mu.Lock()
	if len(s.claims) > 0 {
		s.mu.Unlock()
		return nil, func() {}, true
	}
	if _, exists := s.pending[requestID]; exists {
		s.mu.Unlock()
		return nil, func() {}, false
	}
	s.pending[requestID] = response
	s.mu.Unlock()
	return response, func() {
		s.mu.Lock()
		delete(s.pending, requestID)
		s.mu.Unlock()
	}, false
}

func (s *Session) deliver(message protocol.Message) {
	s.mu.Lock()
	defer s.mu.Unlock()
	response := s.pending[message.RequestID]
	if response == nil {
		return
	}
	if len(s.claims) > 0 {
		message = localError(message.RequestID, protocol.CodeProfileConflict, "This Chrome profile is connected more than once. Search stopped to avoid mixing profiles.")
	}
	select {
	case response <- message:
	default:
	}
}

func (s *Session) deliverPing() {
	s.mu.Lock()
	response := s.pendingPing
	s.mu.Unlock()
	if response == nil {
		return
	}
	select {
	case response <- struct{}{}:
	default:
	}
}

func (s *Session) write(message protocol.Message) error {
	s.writeMu.Lock()
	defer s.writeMu.Unlock()
	return protocol.WriteFrame(s.out, message)
}

func (s *Session) close() {
	s.closedOnce.Do(func() { close(s.closed) })
}

func (s *Session) shutdown() {
	s.stopOnce.Do(func() { close(s.stop) })
}

func (s *Session) isConflicted() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.claims) > 0
}

func (s *Session) addClaim(id string) {
	if strings.TrimSpace(id) == "" {
		return
	}
	s.mu.Lock()
	s.claims[id] = struct{}{}
	for requestID, pending := range s.pending {
		select {
		case <-pending:
		default:
		}
		select {
		case pending <- localError(requestID, protocol.CodeProfileConflict, "This Chrome profile is connected more than once. Search stopped to avoid mixing profiles."):
		default:
		}
	}
	s.mu.Unlock()
}

func (s *Session) removeClaim(id string) {
	s.mu.Lock()
	delete(s.claims, id)
	s.mu.Unlock()
}

func (s *Session) ID() string { return s.id }

func localError(requestID, code, text string) protocol.Message {
	return protocol.Message{V: protocol.Version, Type: protocol.TypeError, RequestID: requestID, Code: code, Text: text}
}
