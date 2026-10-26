use kwel_protocol::*;
use serde::Deserialize;
#[derive(Deserialize)]
struct Case {
    name: String,
    check: String,
    payload: String,
    accepted: bool,
    normalized: Option<serde_json::Value>,
}
// Historical codec snapshot only. Production Native JSON uses the strict
// per-type codec; this test preserves all 81 original verdicts/normalizations
// without changing their expected values or calling them the current contract.
#[derive(Default, serde::Serialize, Deserialize)]
#[serde(
    remote = "Message",
    default,
    deny_unknown_fields,
    rename_all = "camelCase"
)]
struct GoMessageSnapshot {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    request_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    query: String,
    #[serde(skip_serializing_if = "is_zero")]
    limit: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    profile_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    profile_label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    open_ref: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    items: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<Code>,
    #[serde(rename = "message", skip_serializing_if = "String::is_empty")]
    text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    session_id: String,
    #[serde(skip_serializing_if = "is_false")]
    ok: bool,
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(serde::Serialize, Deserialize)]
struct GoWire(#[serde(with = "GoMessageSnapshot")] Message);
#[test]
fn original_go_protocol_v1_regression_fixtures() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/go-v1.json")).unwrap();
    assert_eq!(cases.len(), 81);
    for case in cases {
        let result = serde_json::from_str::<GoWire>(&case.payload)
            .map_err(Error::from)
            .and_then(|wire| {
                let message = &wire.0;
                match case.check.as_str() {
                    "hello" => message.hello(),
                    "search" => message.search(),
                    "search-result" => message.response("r", false, 100),
                    "open-page" => LocalRequest::Open {
                        v: message.v,
                        request_id: message.request_id.clone(),
                        profile_id: message.profile_id.clone(),
                        open_ref: message.open_ref.clone(),
                    }
                    .validate(),
                    "open-result" => message.response("r", true, 0),
                    _ => message.envelope(),
                }?;
                Ok(wire)
            });
        assert_eq!(result.is_ok(), case.accepted, "{}", case.name);
        if let Ok(wire) = result {
            assert_eq!(
                serde_json::to_value(&wire).unwrap(),
                case.normalized.unwrap(),
                "{}",
                case.name
            );
            let frame = encode_frame(&wire, MAX_MESSAGE_BYTES).unwrap();
            assert_eq!(
                u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize,
                frame.len() - 4
            );
            let decoded: GoWire = serde_json::from_slice(
                &read_frame(&mut frame.as_slice(), MAX_MESSAGE_BYTES).unwrap(),
            )
            .unwrap();
            assert_eq!(decoded.0, wire.0);
        }
    }
}
#[test]
fn framing_rejects_empty_oversize_and_partial_frames() {
    for bytes in [
        vec![0, 0, 0, 0],
        ((MAX_MESSAGE_BYTES + 1) as u32).to_le_bytes().to_vec(),
        vec![3, 0, 0, 0, b'{'],
        vec![1, 0],
    ] {
        assert!(read_frame(&mut bytes.as_slice(), MAX_MESSAGE_BYTES).is_err());
    }
    assert!(encode_frame(&"x".repeat(MAX_MESSAGE_BYTES), MAX_MESSAGE_BYTES).is_err());
    for bytes in [
        vec![0xff, 0xfe],
        b"{}{}".to_vec(),
        br#"{"v":1,"v":1,"type":"ping"}"#.to_vec(),
    ] {
        assert!(decode(&bytes).is_err());
    }
}
#[test]
fn public_protocol_rejects_management_arbitrary_urls_and_unknown_fields() {
    let profile = "00000000-0000-4000-8000-00000000000a";
    for kind in ["uninstall", "doctor", "profile-claim", "profile-release"] {
        assert!(
            serde_json::from_value::<LocalRequest>(
                serde_json::json!({"v":1,"type":kind,"requestId":"r","profileId":profile})
            )
            .is_err()
        );
    }
    assert!(serde_json::from_value::<LocalRequest>(serde_json::json!({"v":1,"type":"open-page","requestId":"r","profileId":profile,"url":"https://example.com"})).is_err());
    assert!(serde_json::from_value::<LocalRequest>(serde_json::json!({"v":1,"type":"search","requestId":"r","profileId":profile,"query":"Kyoto","limit":20,"extra":true})).is_err());
    let request:LocalRequest=serde_json::from_value(serde_json::json!({"v":1,"type":"open-page","requestId":"r","profileId":profile,"openRef":"https://example.com"})).unwrap();
    assert!(request.validate().is_err());
}
#[test]
fn origins_are_exact_and_profile_ids_keep_go_uuid_versions() {
    for origin in [
        format!("chrome-extension://{EXTENSION_ID}/"),
        format!("chrome-extension://{EXTENSION_ID}"),
    ] {
        assert!(valid_origin(&origin, EXTENSION_ID));
    }
    for origin in [
        format!("chrome-extension://{EXTENSION_ID}/path"),
        format!("chrome-extension://{EXTENSION_ID}/?x"),
        format!("chrome-extension://evil@{EXTENSION_ID}/"),
        format!("https://{EXTENSION_ID}/"),
    ] {
        assert!(!valid_origin(&origin, EXTENSION_ID));
    }
    for version in 1..=8 {
        assert!(id(&format!("00000000-0000-{version}000-8000-00000000000a")));
    }
    for invalid in [
        "00000000-0000-0000-0000-000000000000",
        "work",
        "0000000000004000800000000000000a",
        "00000000-0000-4000-8000-00000000000a/../other",
    ] {
        assert!(!id(invalid));
    }
}
