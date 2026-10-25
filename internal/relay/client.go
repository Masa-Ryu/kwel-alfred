package relay

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"net"
	"strings"
	"syscall"
	"time"

	"com.kwel/alfredbridge/internal/protocol"
)

type ClientError struct {
	Code    string
	Message string
}

func (e *ClientError) Error() string { return e.Message }

func Search(socketPath, profileID, query string, limit int) (protocol.Message, error) {
	requestID, err := newRequestID()
	if err != nil {
		return protocol.Message{}, err
	}
	request := protocol.Message{V: protocol.Version, Type: protocol.TypeSearch, RequestID: requestID, ProfileID: profileID, Query: query, Limit: limit}
	if !protocol.ValidProfileID(profileID) || protocol.ValidateSearch(request) != nil {
		return protocol.Message{}, &ClientError{Code: protocol.CodeInvalidRequest, Message: "This Kwel search request is invalid."}
	}
	response, err := exchange(socketPath, request, SearchTimeout+time.Second)
	if err != nil {
		return protocol.Message{}, clientErrorFromIO(err, "Kwel disconnected before the search finished.")
	}
	if err := protocol.ValidateSearchResult(response, requestID); err != nil || len(response.Items) > limit {
		return protocol.Message{}, &ClientError{Code: protocol.CodeSearchFailed, Message: "Kwel returned an invalid search response."}
	}
	if response.Type == protocol.TypeError {
		return protocol.Message{}, &ClientError{Code: response.Code, Message: userMessage(response.Code, response.Text)}
	}
	return response, nil
}

func Open(socketPath, profileID, openRef string) error {
	requestID, err := newRequestID()
	if err != nil {
		return err
	}
	request := protocol.Message{V: protocol.Version, Type: protocol.TypeOpenPage, RequestID: requestID, ProfileID: profileID, OpenRef: openRef}
	if err := protocol.ValidateOpenPage(request); err != nil {
		return &ClientError{Code: protocol.CodeInvalidRequest, Message: "This Kwel result cannot be opened."}
	}
	response, err := exchange(socketPath, request, SearchTimeout+time.Second)
	if err != nil {
		return clientErrorFromIO(err, "Kwel disconnected before the page could be opened.")
	}
	if err := protocol.ValidateOpenResult(response, requestID); err != nil {
		return &ClientError{Code: protocol.CodeSearchFailed, Message: "Kwel returned an invalid open response."}
	}
	if response.Type == protocol.TypeError {
		return &ClientError{Code: response.Code, Message: userMessage(response.Code, response.Text)}
	}
	return nil
}

func ProbeProfile(socketPath, profileID string) (Profile, error) {
	if !protocol.ValidProfileID(profileID) {
		return Profile{}, &ClientError{Code: protocol.CodeInvalidRequest, Message: "This Kwel profile identifier is invalid."}
	}
	response, err := exchange(socketPath, protocol.Message{V: protocol.Version, Type: protocol.TypeProfileProbe, ProfileID: profileID}, ProbeTimeout+500*time.Millisecond)
	if err != nil {
		return Profile{}, clientErrorFromIO(err, "Kwel profile did not respond to a connection check.")
	}
	if response.Type == protocol.TypeError {
		if !validLocalError(response) {
			return Profile{}, &ClientError{Code: protocol.CodeDisconnected, Message: "Kwel profile did not respond to a connection check."}
		}
		return Profile{}, &ClientError{Code: response.Code, Message: userMessage(response.Code, response.Text)}
	}
	if response.Type != protocol.TypeProfileInfo || response.ProfileID != profileID || strings.TrimSpace(response.ProfileLabel) == "" {
		return Profile{}, &ClientError{Code: protocol.CodeDisconnected, Message: "Kwel profile returned an invalid connection response."}
	}
	return Profile{ID: response.ProfileID, Label: response.ProfileLabel}, nil
}

func Control(socketPath string, request protocol.Message) (protocol.Message, error) {
	return exchange(socketPath, request, MaxControlTimeout)
}

const MaxControlTimeout = 3 * time.Second

func exchange(socketPath string, request protocol.Message, timeout time.Duration) (protocol.Message, error) {
	connection, err := net.DialTimeout("unix", socketPath, 250*time.Millisecond)
	if err != nil {
		return protocol.Message{}, err
	}
	defer connection.Close()
	_ = connection.SetDeadline(time.Now().Add(timeout))
	if err := protocol.WriteFrame(connection, request); err != nil {
		return protocol.Message{}, err
	}
	payload, err := protocol.ReadFrame(connection)
	if err != nil {
		return protocol.Message{}, err
	}
	response, err := protocol.Decode(payload)
	if err != nil || protocol.ValidateEnvelope(response) != nil {
		return protocol.Message{}, errors.New("local profile returned an invalid response")
	}
	return response, nil
}

func validLocalError(message protocol.Message) bool {
	return message.Type == protocol.TypeError && message.RequestID == "" && message.Code != "" && message.Text != ""
}

func clientErrorFromIO(err error, fallback string) *ClientError {
	if errors.Is(err, syscall.ENOENT) || errors.Is(err, syscall.ECONNREFUSED) {
		return &ClientError{Code: protocol.CodeDisconnected, Message: "The configured Kwel profile is not connected. Check Alfred Integration in that Chrome profile."}
	}
	if timeout, ok := err.(net.Error); ok && timeout.Timeout() {
		return &ClientError{Code: protocol.CodeTimeout, Message: "Kwel took too long to respond. Try again."}
	}
	return &ClientError{Code: protocol.CodeDisconnected, Message: fallback}
}

func userMessage(code, message string) string {
	switch code {
	case protocol.CodeHostNotFound:
		return "Kwel is connecting to Alfred. Try this search again in a moment."
	case protocol.CodeDisconnected:
		return "The configured Kwel profile is disconnected. Check Chrome and Kwel, then retry."
	case protocol.CodeTimeout:
		return "Kwel took too long to respond. Try again."
	case protocol.CodeProfileConflict:
		return "This Kwel profile is connected more than once. Search stopped to avoid mixing profiles."
	case protocol.CodePermissionRequired:
		return "Enable Alfred integration in Kwel Settings to use this profile."
	case protocol.CodeIncompatible:
		return "This Kwel version is not compatible with the Alfred Workflow. Update Kwel and retry."
	case protocol.CodeResultExpired:
		return "This Kwel result has expired. Search again."
	case protocol.CodeOpenFailed:
		return "Kwel could not open this result. Search again."
	case protocol.CodeSearchFailed:
		return "Kwel could not complete this search. Try again."
	default:
		if message != "" {
			return message
		}
		return "Kwel search is unavailable. Check Chrome and Kwel, then retry."
	}
}

func newRequestID() (string, error) {
	var value [16]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", fmt.Errorf("create request identifier: %w", err)
	}
	return "req-" + hex.EncodeToString(value[:]), nil
}
