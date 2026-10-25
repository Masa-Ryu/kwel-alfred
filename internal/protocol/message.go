package protocol

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"regexp"
	"strings"
	"unicode/utf16"
	"unicode/utf8"
)

const (
	Version           = 1
	MaxMessageBytes   = 1_000_000
	MaxQueryCodeUnits = 200
	MaxSearchResults  = 100
	MaxRequestIDSize  = 100
	MaxProfileLabel   = 64
)

const (
	TypeHello        = "hello"
	TypeHelloAck     = "hello-ack"
	TypeSearch       = "search"
	TypeSearchResult = "search-result"
	TypeOpenPage     = "open-page"
	TypeOpenResult   = "open-result"
	TypePing         = "ping"
	TypePong         = "pong"
	TypeError        = "error"
	TypeProfileProbe = "profile-probe"
	TypeProfileInfo  = "profile-info"
	TypeProfileClaim = "profile-claim"
	TypeProfileFree  = "profile-release"
	TypeProfileAck   = "profile-ack"
	TypeDoctor       = "doctor"
	TypeDoctorResult = "doctor-result"
	TypeUninstall    = "uninstall"
	TypeUninstallAck = "uninstall-ack"
)

const (
	CodeHostNotFound       = "HOST_NOT_FOUND"
	CodeDisconnected       = "DISCONNECTED"
	CodeTimeout            = "TIMEOUT"
	CodeProfileConflict    = "PROFILE_CONFLICT"
	CodePermissionRequired = "PERMISSION_REQUIRED"
	CodeIncompatible       = "INCOMPATIBLE_VERSION"
	CodeSearchFailed       = "SEARCH_FAILED"
	CodeOpenFailed         = "OPEN_FAILED"
	CodeResultExpired      = "RESULT_EXPIRED"
	CodeInvalidRequest     = "INVALID_REQUEST"
)

var (
	ErrInvalidJSON = errors.New("invalid JSON message")
	ErrInvalidMsg  = errors.New("invalid protocol message")
)

type Item struct {
	Title   string `json:"title"`
	URL     string `json:"url"`
	Domain  string `json:"domain"`
	OpenRef string `json:"openRef"`
}

type Message struct {
	V            int    `json:"v"`
	Type         string `json:"type"`
	RequestID    string `json:"requestId,omitempty"`
	Query        string `json:"query,omitempty"`
	Limit        int    `json:"limit,omitempty"`
	ProfileID    string `json:"profileId,omitempty"`
	ProfileLabel string `json:"profileLabel,omitempty"`
	OpenRef      string `json:"openRef,omitempty"`
	Items        []Item `json:"items,omitempty"`
	HasMore      *bool  `json:"hasMore,omitempty"`
	Code         string `json:"code,omitempty"`
	Text         string `json:"message,omitempty"`
	SessionID    string `json:"sessionId,omitempty"`
	OK           bool   `json:"ok,omitempty"`
}

var profileIDPattern = regexp.MustCompile(`^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$`)

func ValidProfileID(value string) bool { return profileIDPattern.MatchString(value) }

func StringCodeUnits(value string) int { return len(utf16.Encode([]rune(value))) }

func Decode(data []byte) (Message, error) {
	if len(data) == 0 || len(data) > MaxMessageBytes || !utf8.Valid(data) {
		return Message{}, ErrInvalidJSON
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	var message Message
	if err := decoder.Decode(&message); err != nil {
		return Message{}, fmt.Errorf("%w: %v", ErrInvalidJSON, err)
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return Message{}, ErrInvalidJSON
	}
	return message, nil
}

func ValidateEnvelope(message Message) error {
	if message.V != Version {
		return fmt.Errorf("%w: protocol version %d", ErrInvalidMsg, message.V)
	}
	if message.Type == "" {
		return fmt.Errorf("%w: missing type", ErrInvalidMsg)
	}
	return nil
}

func ValidateProfileHello(message Message) error {
	if err := ValidateEnvelope(message); err != nil {
		return err
	}
	if message.Type != TypeHello || !ValidProfileID(message.ProfileID) {
		return fmt.Errorf("%w: unexpected hello", ErrInvalidMsg)
	}
	if StringCodeUnits(message.ProfileLabel) > MaxProfileLabel || strings.ContainsAny(message.ProfileLabel, "\r\n") {
		return fmt.Errorf("%w: profile label is invalid", ErrInvalidMsg)
	}
	return nil
}

func ValidateSearch(message Message) error {
	if err := ValidateEnvelope(message); err != nil {
		return err
	}
	if message.Type != TypeSearch || StringCodeUnits(message.RequestID) == 0 || StringCodeUnits(message.RequestID) > MaxRequestIDSize {
		return fmt.Errorf("%w: search request is incomplete", ErrInvalidMsg)
	}
	if StringCodeUnits(message.Query) > MaxQueryCodeUnits {
		return fmt.Errorf("%w: query length is invalid", ErrInvalidMsg)
	}
	if message.Limit < 1 || message.Limit > MaxSearchResults {
		return fmt.Errorf("%w: result limit is invalid", ErrInvalidMsg)
	}
	return nil
}

func ValidateOpenPage(message Message) error {
	if err := ValidateEnvelope(message); err != nil {
		return err
	}
	if message.Type != TypeOpenPage || StringCodeUnits(message.RequestID) == 0 || StringCodeUnits(message.RequestID) > MaxRequestIDSize || !ValidProfileID(message.ProfileID) || !ValidProfileID(message.OpenRef) {
		return fmt.Errorf("%w: open request is incomplete", ErrInvalidMsg)
	}
	return nil
}

func ValidateSearchResult(message Message, expectedRequestID string) error {
	if err := ValidateEnvelope(message); err != nil {
		return err
	}
	if message.Type == TypeError {
		if message.RequestID != expectedRequestID || !validErrorCode(message.Code) {
			return fmt.Errorf("%w: error response is incomplete", ErrInvalidMsg)
		}
		return nil
	}
	if message.Type != TypeSearchResult || message.RequestID != expectedRequestID || len(message.Items) > MaxSearchResults || message.HasMore == nil {
		return fmt.Errorf("%w: search result does not match request", ErrInvalidMsg)
	}
	for _, item := range message.Items {
		parsed, err := url.Parse(item.URL)
		if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Hostname() == "" {
			return fmt.Errorf("%w: result URL must be HTTP or HTTPS", ErrInvalidMsg)
		}
		if strings.TrimSpace(item.Title) == "" || !ValidProfileID(item.OpenRef) {
			return fmt.Errorf("%w: result title or open reference is invalid", ErrInvalidMsg)
		}
	}
	return nil
}

func ValidateOpenResult(message Message, expectedRequestID string) error {
	if err := ValidateEnvelope(message); err != nil {
		return err
	}
	if message.Type == TypeError {
		if message.RequestID != expectedRequestID || !validErrorCode(message.Code) {
			return fmt.Errorf("%w: open error response is incomplete", ErrInvalidMsg)
		}
		return nil
	}
	if message.Type != TypeOpenResult || message.RequestID != expectedRequestID || !message.OK {
		return fmt.Errorf("%w: open response does not match request", ErrInvalidMsg)
	}
	return nil
}

func validErrorCode(code string) bool {
	switch code {
	case "BAD_REQUEST", CodeIncompatible, "NOT_READY", CodeResultExpired, CodeOpenFailed, CodeSearchFailed, CodeProfileConflict:
		return true
	case CodeHostNotFound, CodeDisconnected, CodeTimeout, CodePermissionRequired, CodeInvalidRequest:
		return true
	default:
		return false
	}
}
