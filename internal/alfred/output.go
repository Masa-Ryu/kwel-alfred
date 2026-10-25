package alfred

import (
	"encoding/json"
	"fmt"
	"net/url"
	"strings"

	"com.kwel/alfredbridge/internal/protocol"
)

type Output struct {
	SkipKnowledge bool   `json:"skipknowledge"`
	Items         []Item `json:"items"`
}

type Item struct {
	Title     string            `json:"title"`
	Subtitle  string            `json:"subtitle,omitempty"`
	Arg       string            `json:"arg,omitempty"`
	Valid     bool              `json:"valid"`
	Variables map[string]string `json:"variables,omitempty"`
}

func Results(message protocol.Message, profileID string) Output {
	if message.Type == protocol.TypeError {
		return Notice(message.Text)
	}
	if len(message.Items) == 0 {
		return Notice("No matching Kwel pages")
	}
	items := make([]Item, 0, len(message.Items)+1)
	for _, result := range message.Items {
		domain := strings.TrimSpace(result.Domain)
		if domain == "" {
			if parsed, err := url.Parse(result.URL); err == nil {
				domain = parsed.Hostname()
			}
		}
		items = append(items, Item{
			Title:    result.Title,
			Subtitle: domain,
			Arg:      result.OpenRef,
			Valid:    true,
			Variables: map[string]string{
				"KWEL_RESULT_PROFILE_ID": profileID,
			},
		})
	}
	if message.HasMore != nil && *message.HasMore {
		items = append(items, Item{
			Title: fmt.Sprintf("Showing the first %d results. Refine your search for more.", len(message.Items)),
			Valid: false,
		})
	}
	return Output{SkipKnowledge: true, Items: items}
}

func ProfileConfigurationRequired() Output {
	output := Notice("Configure the Kwel Profile ID")
	output.Items[0].Subtitle = "Paste the Profile ID from Kwel Settings → Alfred Integration into Workflow Configuration."
	return output
}

func Notice(text string) Output {
	text = strings.TrimSpace(text)
	if text == "" {
		text = "Kwel search is unavailable. Check Chrome and Kwel, then retry."
	}
	return Output{SkipKnowledge: true, Items: []Item{{Title: text, Valid: false}}}
}

func Encode(output Output) ([]byte, error) {
	return json.Marshal(output)
}
