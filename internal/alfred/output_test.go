package alfred

import (
	"encoding/json"
	"fmt"
	"testing"

	"com.kwel/alfredbridge/internal/protocol"
)

const (
	profileID = "550e8400-e29b-41d4-a716-446655440000"
	openRef   = "111e8400-e29b-41d4-a716-446655440001"
)

func TestResultsPreserveKwelOrderAndBindOpaqueReferencesToSearchProfile(t *testing.T) {
	output := Results(protocol.Message{
		Type: protocol.TypeSearchResult,
		Items: []protocol.Item{
			{Title: "Second-ranked", URL: "https://second.example/path", Domain: "second.example", OpenRef: openRef},
			{Title: "First-ranked", URL: "http://first.example/", OpenRef: "222e8400-e29b-41d4-a716-446655440002"},
		},
	}, profileID)
	if !output.SkipKnowledge || len(output.Items) != 2 || output.Items[0].Title != "Second-ranked" || output.Items[1].Title != "First-ranked" {
		t.Fatalf("result order changed: %#v", output)
	}
	for _, item := range output.Items {
		if !item.Valid || !protocol.ValidProfileID(item.Arg) || item.Variables["KWEL_RESULT_PROFILE_ID"] != profileID || len(item.Variables) != 1 {
			t.Fatalf("result is not bound to its search profile and opaque reference: %#v", item)
		}
	}
	if output.Items[0].Arg != openRef || output.Items[1].Subtitle != "first.example" {
		t.Fatalf("open reference or display domain changed: %#v", output.Items)
	}
	encoded, err := Encode(output)
	if err != nil {
		t.Fatal(err)
	}
	var parsed map[string]any
	if err := json.Unmarshal(encoded, &parsed); err != nil {
		t.Fatal(err)
	}
	if parsed["skipknowledge"] != true {
		t.Fatalf("skipknowledge missing: %s", encoded)
	}
	for _, row := range parsed["items"].([]any) {
		for _, forbidden := range []string{"uid", "mods", "action", "match"} {
			if _, exists := row.(map[string]any)[forbidden]; exists {
				t.Fatalf("unexpected %s in search results: %s", forbidden, encoded)
			}
		}
	}
}

func TestEmptyErrorAndConfigurationNoticesAreOneDisabledItem(t *testing.T) {
	for _, output := range []Output{
		Results(protocol.Message{Type: protocol.TypeSearchResult}, profileID),
		Results(protocol.Message{Type: protocol.TypeError, Text: "Search unavailable."}, profileID),
		Notice("No connection."),
		ProfileConfigurationRequired(),
	} {
		if !output.SkipKnowledge || len(output.Items) != 1 || output.Items[0].Valid || output.Items[0].Arg != "" || len(output.Items[0].Variables) != 0 {
			t.Fatalf("notice should be one non-actionable item: %#v", output)
		}
	}
	if got := ProfileConfigurationRequired().Items[0].Title; got != "Configure the Kwel Profile ID" {
		t.Fatalf("unexpected configuration notice: %q", got)
	}
}

func TestTwentySearchResultsRemainInKwelOrder(t *testing.T) {
	results := make([]protocol.Item, 20)
	for index := range results {
		results[index] = protocol.Item{Title: fmt.Sprintf("Result %02d", 20-index), URL: "https://example.com/", Domain: "example.com", OpenRef: fmt.Sprintf("333e8400-e29b-41d4-a716-%012d", index)}
	}
	output := Results(protocol.Message{Type: protocol.TypeSearchResult, Items: results}, profileID)
	if len(output.Items) != 20 {
		t.Fatalf("got %d results, want 20", len(output.Items))
	}
	for index, item := range output.Items {
		if item.Title != results[index].Title || item.Arg != results[index].OpenRef {
			t.Fatalf("result %d was reordered or changed: %#v", index, item)
		}
	}
}
