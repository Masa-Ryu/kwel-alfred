package main

import (
	"errors"
	"net"
	"path/filepath"
	"testing"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
	"com.kwel/alfredbridge/internal/relay"
	"com.kwel/alfredbridge/internal/testutil"
)

func TestOpenActivatesChromeOnlyAfterKwelAcknowledgesTheSelectedResult(t *testing.T) {
	activationFailure := errors.New("activation failed")
	for _, test := range []struct {
		name          string
		responseCode  string
		disconnect    bool
		activationErr error
	}{
		{name: "successful open activates Chrome"},
		{name: "expired reference does not activate", responseCode: protocol.CodeResultExpired},
		{name: "disconnected profile does not activate", disconnect: true},
		{name: "activation error is reported after opening", activationErr: activationFailure},
	} {
		t.Run(test.name, func(t *testing.T) {
			directory := testutil.SocketDir(t)
			profileID := "00000000-0000-4000-8000-00000000000a"
			openRef := "00000000-0000-4000-8000-000000000101"
			socketPath := filepath.Join(directory, profileID+".sock")
			listener, err := net.Listen("unix", socketPath)
			if err != nil {
				t.Fatal(err)
			}
			requests := make(chan protocol.Message, 1)
			allowResponse := make(chan struct{})
			stop := make(chan struct{})
			serverDone := make(chan struct{})
			go func() {
				defer close(serverDone)
				connection, err := listener.Accept()
				if err != nil {
					return
				}
				defer connection.Close()
				_ = connection.SetDeadline(time.Now().Add(3 * time.Second))
				payload, err := protocol.ReadFrame(connection)
				if err != nil {
					t.Errorf("read open request: %v", err)
					return
				}
				request, err := protocol.Decode(payload)
				if err != nil {
					t.Errorf("decode open request: %v", err)
					return
				}
				requests <- request
				select {
				case <-allowResponse:
				case <-stop:
					return
				}
				if test.disconnect {
					return
				}
				response := protocol.Message{V: 1, Type: protocol.TypeOpenResult, RequestID: request.RequestID, OK: true}
				if test.responseCode != "" {
					response = protocol.Message{V: 1, Type: protocol.TypeError, RequestID: request.RequestID, Code: test.responseCode, Text: "Result unavailable."}
				}
				if err := protocol.WriteFrame(connection, response); err != nil {
					t.Errorf("write open response: %v", err)
				}
			}()
			t.Cleanup(func() {
				close(stop)
				_ = listener.Close()
				select {
				case <-serverDone:
				case <-time.After(3 * time.Second):
					t.Error("mock Native Host did not stop")
				}
			})
			activated := make(chan struct{}, 1)
			finished := make(chan error, 1)
			go func() {
				finished <- openResultAndActivate(socketPath, profileID, openRef, func() error {
					activated <- struct{}{}
					return test.activationErr
				})
			}()
			select {
			case request := <-requests:
				if request.Type != protocol.TypeOpenPage || request.ProfileID != profileID || request.OpenRef != openRef {
					t.Fatalf("selected result or profile changed: %#v", request)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("open request did not reach its profile")
			}
			select {
			case <-activated:
				t.Fatal("Chrome activated before Kwel acknowledged opening the page")
			default:
			}
			close(allowResponse)
			select {
			case err := <-finished:
				if test.responseCode != "" || test.disconnect {
					var clientErr *relay.ClientError
					if !errors.As(err, &clientErr) {
						t.Fatalf("failed open lost its error: %v", err)
					}
					if test.responseCode != "" && clientErr.Code != test.responseCode {
						t.Fatalf("wrong open error: %v", err)
					}
					select {
					case <-activated:
						t.Fatal("Chrome activated after a failed open")
					default:
					}
				} else {
					if !errors.Is(err, test.activationErr) {
						t.Fatalf("activation result = %v; want %v", err, test.activationErr)
					}
					select {
					case <-activated:
					default:
						t.Fatal("successful open did not activate Chrome")
					}
				}
			case <-time.After(2 * time.Second):
				t.Fatal("open operation did not finish")
			}
		})
	}
}
