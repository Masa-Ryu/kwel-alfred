use kwel_alfred::*;
use kwel_protocol::{Code, Item};
#[test]
fn results_preserve_ranking_and_only_open_reference_and_result_profile() {
    let profile = "00000000-0000-4000-8000-00000000000a";
    let items = (0..20)
        .map(|index| Item {
            title: format!("Result {}", 20 - index),
            url: "https://example.com/page".into(),
            domain: String::new(),
            open_ref: format!("00000000-0000-4000-8000-{index:012}"),
        })
        .collect();
    let output = results(items, true, profile);
    assert!(output.skipknowledge);
    assert_eq!(output.items.len(), 21);
    assert_eq!(output.items[0].title, "Result 20");
    assert_eq!(output.items[19].title, "Result 1");
    assert_eq!(output.items[0].subtitle, "example.com");
    assert_eq!(output.items[0].variables["KWEL_RESULT_PROFILE_ID"], profile);
    let value = serde_json::to_value(output).unwrap();
    for item in value["items"].as_array().unwrap() {
        for forbidden in ["uid", "mods", "action", "match"] {
            assert!(item.get(forbidden).is_none());
        }
    }
    assert_eq!(
        value["items"][20]["title"],
        "Showing the first 20 results. Refine your search for more."
    );
    assert_eq!(value["items"][20]["valid"], false);
}
#[test]
fn notices_are_single_disabled_rows() {
    for output in [
        notice("Unavailable"),
        configuration_required(),
        results(vec![], false, "unused"),
    ] {
        assert_eq!(output.items.len(), 1);
        assert!(!output.items[0].valid);
        assert!(output.items[0].arg.is_empty());
        assert!(output.items[0].variables.is_empty());
    }
}
#[test]
fn connection_deadline_survives_query_edits_and_stops() {
    let now = 1_700_000_000_000;
    let start = wait(Code::Disconnected, "", now);
    assert_eq!(start.rerun, Some(1.0));
    let deadline = start.variables[WAIT_VARIABLE].clone();
    assert_eq!(
        wait(Code::Disconnected, &deadline, now + 10_000).variables[WAIT_VARIABLE],
        deadline
    );
    let end = wait(Code::Disconnected, &deadline, now + WAIT_MILLIS);
    assert!(end.rerun.is_none());
    assert_eq!(end.variables[WAIT_VARIABLE], "");
    assert_eq!(end.items[0].title, Code::Disconnected.message());
    assert_eq!(
        wait(Code::Disconnected, "invalid", now).variables[WAIT_VARIABLE],
        deadline
    );
    assert_eq!(
        wait(Code::Disconnected, &u64::MAX.to_string(), now).variables[WAIT_VARIABLE],
        deadline
    );
}
