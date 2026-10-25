package relay

import (
	"context"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"sync"
	"syscall"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
	"com.kwel/alfredbridge/internal/securefs"
)

var ErrProfileConflict = errors.New("profile id is already owned by another Native Messaging host")

type SocketServer struct {
	Path    string
	Session *Session
}

// Serve takes ownership of one profile socket. When ready is supplied it receives
// the bind result before Serve begins accepting local requests.
func (s SocketServer) Serve(done <-chan struct{}, ready ...chan<- error) error {
	var readiness chan<- error
	if len(ready) > 0 {
		readiness = ready[0]
	}
	listener, fileInfo, err := s.listen()
	if err != nil {
		if errors.Is(err, os.ErrExist) {
			if claimErr := s.claimExisting(); claimErr == nil {
				err = ErrProfileConflict
			}
		}
		if readiness != nil {
			readiness <- err
		}
		return err
	}
	if readiness != nil {
		readiness <- nil
	}
	return s.serveOwner(listener, fileInfo, done)
}

func (s SocketServer) listen() (*net.UnixListener, os.FileInfo, error) {
	directory := filepath.Dir(s.Path)
	if err := securefs.VerifyPrivateDir(directory); err != nil {
		return nil, nil, err
	}
	if info, err := os.Lstat(s.Path); err == nil {
		if err := securefs.VerifyOwnedSocket(s.Path); err != nil {
			return nil, nil, err
		}
		connection, dialErr := net.DialTimeout("unix", s.Path, 150*time.Millisecond)
		if dialErr == nil {
			_ = connection.Close()
			return nil, nil, os.ErrExist
		}
		if !errors.Is(dialErr, syscall.ECONNREFUSED) {
			return nil, nil, dialErr
		}
		current, statErr := os.Lstat(s.Path)
		if statErr != nil {
			if errors.Is(statErr, os.ErrNotExist) {
				return s.bind()
			}
			return nil, nil, statErr
		}
		if !os.SameFile(info, current) {
			return nil, nil, fmt.Errorf("%w: stale socket path changed during cleanup", securefs.ErrUnsafePath)
		}
		if err := os.Remove(s.Path); err != nil && !errors.Is(err, os.ErrNotExist) {
			return nil, nil, err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, nil, err
	}
	return s.bind()
}

func (s SocketServer) bind() (*net.UnixListener, os.FileInfo, error) {
	listener, err := net.ListenUnix("unix", &net.UnixAddr{Name: s.Path, Net: "unix"})
	if err != nil {
		if errors.Is(err, os.ErrExist) || isAddressInUse(err) {
			return nil, nil, os.ErrExist
		}
		return nil, nil, err
	}
	if err := os.Chmod(s.Path, 0o600); err != nil {
		_ = listener.Close()
		return nil, nil, err
	}
	info, err := os.Lstat(s.Path)
	if err != nil {
		_ = listener.Close()
		return nil, nil, err
	}
	if err := securefs.VerifyOwnedSocket(s.Path); err != nil {
		_ = listener.Close()
		return nil, nil, err
	}
	return listener, info, nil
}

func (s SocketServer) serveOwner(listener *net.UnixListener, ownedFile os.FileInfo, done <-chan struct{}) error {
	finished := make(chan struct{})
	defer func() {
		close(finished)
		_ = listener.Close()
		if current, err := os.Lstat(s.Path); err == nil && os.SameFile(ownedFile, current) {
			_ = os.Remove(s.Path)
		}
	}()
	var wait sync.WaitGroup
	defer wait.Wait()
	go func() {
		select {
		case <-done:
			_ = listener.Close()
		case <-finished:
		}
	}()
	for {
		connection, err := listener.AcceptUnix()
		if err != nil {
			return err
		}
		wait.Add(1)
		go func() {
			defer wait.Done()
			s.serveConnection(connection)
		}()
	}
}

func (s SocketServer) serveConnection(connection *net.UnixConn) {
	defer connection.Close()
	_ = connection.SetDeadline(time.Now().Add(SearchTimeout + 2*time.Second))
	if err := securefs.VerifyPrivateDir(filepath.Dir(s.Path)); err != nil {
		return
	}
	payload, err := protocol.ReadFrame(connection)
	if err != nil {
		return
	}
	message, err := protocol.Decode(payload)
	if err != nil || protocol.ValidateEnvelope(message) != nil {
		_ = protocol.WriteFrame(connection, localError("", protocol.CodeInvalidRequest, "Invalid local request."))
		return
	}
	switch message.Type {
	case protocol.TypeProfileClaim:
		s.Session.addClaim(message.SessionID)
		_ = protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeProfileAck, SessionID: message.SessionID})
	case protocol.TypeProfileFree:
		s.Session.removeClaim(message.SessionID)
		_ = protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeProfileAck, SessionID: message.SessionID})
	case protocol.TypeDoctor:
		code := "CONNECTED"
		if s.Session.isConflicted() {
			code = protocol.CodeProfileConflict
		}
		_ = protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeDoctorResult, Code: code, Text: s.Session.version})
	case protocol.TypeUninstall:
		_ = protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeUninstallAck})
		s.Session.shutdown()
	case protocol.TypeProfileProbe:
		if message.ProfileID != "" && message.ProfileID != s.Session.profileID {
			_ = protocol.WriteFrame(connection, localError("", protocol.CodeInvalidRequest, "The requested profile does not match this socket."))
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), ProbeTimeout)
		defer cancel()
		if err := s.Session.Probe(ctx); err != nil {
			var clientError *ClientError
			if errors.As(err, &clientError) {
				_ = protocol.WriteFrame(connection, localError("", clientError.Code, clientError.Message))
			} else {
				_ = protocol.WriteFrame(connection, localError("", protocol.CodeDisconnected, "Kwel profile did not respond."))
			}
			return
		}
		_ = protocol.WriteFrame(connection, protocol.Message{V: protocol.Version, Type: protocol.TypeProfileInfo, ProfileID: s.Session.profileID, ProfileLabel: s.Session.profileLabel, Text: s.Session.version})
	case protocol.TypeSearch:
		if message.ProfileID != s.Session.profileID {
			_ = protocol.WriteFrame(connection, localError(message.RequestID, protocol.CodeInvalidRequest, "The requested profile does not match this socket."))
			return
		}
		if err := protocol.ValidateSearch(message); err != nil {
			_ = protocol.WriteFrame(connection, localError(message.RequestID, protocol.CodeInvalidRequest, "The search request is invalid."))
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), SearchTimeout)
		defer cancel()
		response := s.Session.Request(ctx, message)
		_ = protocol.WriteFrame(connection, response)
	case protocol.TypeOpenPage:
		if message.ProfileID != s.Session.profileID {
			_ = protocol.WriteFrame(connection, localError(message.RequestID, protocol.CodeInvalidRequest, "The requested profile does not match this socket."))
			return
		}
		if err := protocol.ValidateOpenPage(message); err != nil {
			_ = protocol.WriteFrame(connection, localError(message.RequestID, protocol.CodeInvalidRequest, "The open request is invalid."))
			return
		}
		ctx, cancel := context.WithTimeout(context.Background(), SearchTimeout)
		defer cancel()
		response := s.Session.Open(ctx, message)
		_ = protocol.WriteFrame(connection, response)
	default:
		_ = protocol.WriteFrame(connection, localError(message.RequestID, protocol.CodeInvalidRequest, "Unsupported local request."))
	}
}

func (s SocketServer) claimExisting() error {
	connection, err := net.DialTimeout("unix", s.Path, 300*time.Millisecond)
	if err != nil {
		return err
	}
	defer connection.Close()
	_ = connection.SetDeadline(time.Now().Add(500 * time.Millisecond))
	claim := protocol.Message{V: protocol.Version, Type: protocol.TypeProfileClaim, SessionID: s.Session.ID()}
	if err := protocol.WriteFrame(connection, claim); err != nil {
		return err
	}
	payload, err := protocol.ReadFrame(connection)
	if err != nil {
		return err
	}
	ack, err := protocol.Decode(payload)
	if err != nil || ack.Type != protocol.TypeProfileAck || ack.SessionID != claim.SessionID {
		return errors.New("profile conflict registration failed")
	}
	return nil
}

func isAddressInUse(err error) bool { return errors.Is(err, syscall.EADDRINUSE) }
