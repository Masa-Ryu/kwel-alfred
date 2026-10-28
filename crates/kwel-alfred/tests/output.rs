use kwel_alfred::*;
use kwel_protocol::{Code, Item, Profile};
const PROFILE_A: &str = "00000000-0000-4000-8000-00000000000a";
const PROFILE_B: &str = "00000000-0000-4000-8000-00000000000b";

fn profile(profile_id: &str, profile_label: &str) -> Profile {
    Profile {
        profile_id: profile_id.into(),
        profile_label: profile_label.into(),
        host_version: "1.0.0".into(),
    }
}

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
fn profile_mismatch_requires_another_connected_profile_and_no_configured_match() {
    let other = profile(PROFILE_B, "Private Chrome profile title");
    let configured = profile(PROFILE_A, "Configured Chrome profile");
    assert!(profile_mismatch(PROFILE_A, &[]).is_none());
    assert!(profile_mismatch(PROFILE_A, std::slice::from_ref(&configured)).is_none());
    assert!(profile_mismatch(PROFILE_A, &[configured, other.clone()]).is_none());

    let output = profile_mismatch(PROFILE_A, &[other]).unwrap();
    assert_eq!(output.items[0].title, "Kwel Profile ID does not match");
    assert!(
        output.items[0]
            .subtitle
            .contains("Kwel Settings → Local Connection")
    );
    assert!(!output.items[0].valid);
    let json = serde_json::to_string(&output).unwrap();
    for private_value in [PROFILE_A, PROFILE_B, "Private Chrome profile title"] {
        assert!(!json.contains(private_value));
    }
}

#[test]
fn profile_diagnosis_waits_for_deadline_and_is_rate_limited() {
    let now = 1_700_000_000_000;
    let deadline = (now + WAIT_MILLIS).to_string();
    assert!(!profile_diagnostic_due("", "", now + WAIT_MILLIS));
    assert!(!profile_diagnostic_due(&deadline, "", now));
    assert!(profile_diagnostic_due(&deadline, "", now + WAIT_MILLIS));

    let checked_at = now + WAIT_MILLIS;
    let recent_check = checked_at.to_string();
    assert!(!profile_diagnostic_due(
        &deadline,
        &recent_check,
        checked_at + PROFILE_DIAGNOSTIC_INTERVAL_MILLIS - 1
    ));
    assert!(profile_diagnostic_due(
        &deadline,
        &recent_check,
        checked_at + PROFILE_DIAGNOSTIC_INTERVAL_MILLIS
    ));
    assert!(profile_diagnostic_due(&deadline, "invalid", checked_at));
    assert!(profile_diagnostic_due(
        &deadline,
        &u64::MAX.to_string(),
        checked_at
    ));
}

#[test]
fn only_disconnected_errors_can_start_profile_diagnosis() {
    assert!(can_diagnose_profile_mismatch(Code::Disconnected));
    assert!(can_diagnose_profile_mismatch(Code::HostNotFound));
    for code in [
        Code::Timeout,
        Code::PermissionRequired,
        Code::UnsafePath,
        Code::Busy,
    ] {
        assert!(!can_diagnose_profile_mismatch(code));
    }
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
#[tokio::test]
async fn chrome_activation_waits_for_ack_and_failures_never_activate() {
    use kwel_protocol::{Error, LocalResponse};
    use std::cell::Cell;
    let activated = Cell::new(false);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let opening = open_acknowledged(async { receiver.await.unwrap() }, || {
        activated.set(true);
        Ok(())
    });
    tokio::pin!(opening);
    tokio::select! {
        _ = &mut opening => panic!("opened before Kwel acknowledged"),
        _ = tokio::task::yield_now() => {}
    }
    assert!(!activated.get());
    sender
        .send(Ok(LocalResponse::Open {
            v: 1,
            request_id: "r".into(),
            ok: true,
        }))
        .unwrap();
    opening.await.unwrap();
    assert!(activated.get());
    for response in [
        Err(Error(Code::ResultExpired)),
        Err(Error(Code::Disconnected)),
        Err(Error(Code::Timeout)),
        Ok(LocalResponse::Open {
            v: 1,
            request_id: "r".into(),
            ok: false,
        }),
    ] {
        activated.set(false);
        assert!(
            open_acknowledged(async { response }, || {
                activated.set(true);
                Ok(())
            })
            .await
            .is_err()
        );
        assert!(!activated.get());
    }
}
