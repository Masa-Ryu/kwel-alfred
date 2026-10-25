package protocol

import (
	"bytes"
	"encoding/binary"
	"errors"
	"io"
	"reflect"
	"testing"
)

// Synthetic fixtures only; no user profile or credential is used.
const (
	profileIDA = "00000000-0000-4000-8000-00000000000a"
	openRefA   = "00000000-0000-4000-8000-000000000101"
)

func TestFrameRoundTripAndLimits(t *testing.T) {
	want := Message{V: Version, Type: TypeSearch, RequestID: "req-1", Query: "日本語 symbols !", Limit: 20}
	var buffer bytes.Buffer
	if err := WriteFrame(&buffer, want); err != nil {
		t.Fatal(err)
	}
	frame, err := ReadFrame(&buffer)
	if err != nil {
		t.Fatal(err)
	}
	got, err := Decode(frame)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("message mismatch: got %#v, want %#v", got, want)
	}
	tooLarge := make([]byte, 4)
	binary.LittleEndian.PutUint32(tooLarge, MaxMessageBytes+1)
	if _, err := ReadFrame(bytes.NewReader(tooLarge)); err == nil {
		t.Fatal("oversized frame accepted")
	}
	if err := WriteFrame(io.Discard, struct {
		V       int    `json:"v"`
		Type    string `json:"type"`
		Padding string `json:"padding"`
	}{Version, TypeSearch, string(bytes.Repeat([]byte("x"), MaxMessageBytes))}); err == nil {
		t.Fatal("oversized outbound native message accepted")
	}
	if _, err := ReadFrame(bytes.NewReader([]byte{0, 0, 0, 0})); err == nil {
		t.Fatal("empty frame accepted")
	}
	if _, err := ReadFrame(bytes.NewReader([]byte{3, 0, 0, 0, '{'})); !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("short payload error = %v", err)
	}
}

func TestDecodeRejectsInvalidUTF8TrailingJSONAndUnknownFields(t *testing.T) {
	for _, value := range [][]byte{
		[]byte(`{"v":1,"type":"hello"}{"v":1,"type":"hello"}`),
		[]byte(`{"v":1,"type":"hello","untrusted":true}`),
		{0xff, 0xfe},
	} {
		if _, err := Decode(value); !errors.Is(err, ErrInvalidJSON) {
			t.Errorf("Decode(%q) error = %v", value, err)
		}
	}
}

func TestValidateProfileHelloAndSearchLimits(t *testing.T) {
	hello := Message{V: Version, Type: TypeHello, ProfileID: profileIDA, ProfileLabel: "Work"}
	if err := ValidateProfileHello(hello); err != nil {
		t.Fatal(err)
	}
	if err := ValidateProfileHello(Message{V: Version, Type: TypeHello, ProfileID: profileIDA}); err != nil {
		t.Fatalf("Profile ID hello must not require a profile name: %v", err)
	}
	for _, invalid := range []Message{
		{V: Version, Type: TypeHello, ProfileID: "work", ProfileLabel: "Work"},
		{V: Version, Type: TypeHello, ProfileID: profileIDA, ProfileLabel: "line\nbreak"},
		{V: Version, Type: TypeHello, ProfileID: profileIDA, ProfileLabel: string(bytes.Repeat([]byte("x"), MaxProfileLabel+1))},
	} {
		if ValidateProfileHello(invalid) == nil {
			t.Errorf("invalid profile hello accepted: %#v", invalid)
		}
	}
	for _, query := range []string{"日本語 ⚙", ""} {
		if err := ValidateSearch(Message{V: Version, Type: TypeSearch, RequestID: "r", Query: query, Limit: 100}); err != nil {
			t.Fatalf("valid query %q rejected: %v", query, err)
		}
	}
	if err := ValidateSearch(Message{V: Version, Type: TypeSearch, RequestID: "r", Query: "x", Limit: 101}); err == nil {
		t.Fatal("limit above 100 was accepted")
	}
	if err := ValidateSearch(Message{V: Version, Type: TypeSearch, RequestID: "r", Query: string(bytes.Repeat([]byte("あ"), 201)), Limit: 20}); err == nil {
		t.Fatal("query above 200 JavaScript code units was accepted")
	}
	if err := ValidateSearch(Message{V: Version, Type: TypeSearch, RequestID: "r", Query: string(bytes.Repeat([]byte("😀"), 101)), Limit: 20}); err == nil {
		t.Fatal("surrogate pairs above 200 JavaScript code units were accepted")
	}
}

func TestValidateSearchResultAndOpenMessages(t *testing.T) {
	hasMore := false
	valid := Message{V: Version, Type: TypeSearchResult, RequestID: "r", HasMore: &hasMore, Items: []Item{{Title: "One", URL: "https://example.com/a", Domain: "example.com", OpenRef: openRefA}}}
	if err := ValidateSearchResult(valid, "r"); err != nil {
		t.Fatal(err)
	}
	for _, item := range []Item{
		{Title: "Bad URL", URL: "javascript:alert(1)", OpenRef: openRefA},
		{Title: "Bad ref", URL: "https://example.com", OpenRef: "https://example.com"},
	} {
		message := valid
		message.Items = []Item{item}
		if ValidateSearchResult(message, "r") == nil {
			t.Errorf("unsafe result accepted: %#v", item)
		}
	}
	withoutMore := valid
	withoutMore.HasMore = nil
	if ValidateSearchResult(withoutMore, "r") == nil {
		t.Fatal("search result without hasMore was accepted")
	}

	open := Message{V: Version, Type: TypeOpenPage, RequestID: "open-1", ProfileID: profileIDA, OpenRef: openRefA}
	if err := ValidateOpenPage(open); err != nil {
		t.Fatal(err)
	}
	if err := ValidateOpenResult(Message{V: Version, Type: TypeOpenResult, RequestID: "open-1", OK: true}, "open-1"); err != nil {
		t.Fatal(err)
	}
	if err := ValidateOpenResult(Message{V: Version, Type: TypeOpenResult, RequestID: "open-1"}, "open-1"); err == nil {
		t.Fatal("failed open result accepted as success")
	}
}
