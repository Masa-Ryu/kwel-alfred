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
#[test]
fn go_protocol_v1_fixtures() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/go-v1.json")).unwrap();
    assert!(cases.len() >= 30);
    for case in cases {
        let result = decode(case.payload.as_bytes()).and_then(|message| {
            match case.check.as_str() {
                "hello" => message.hello(),
                "search" => message.search(),
                "search-result" => message.response("r", false, 100),
                "open-page" => message.open(),
                "open-result" => message.response("r", true, 0),
                _ => message.envelope(),
            }?;
            Ok(message)
        });
        assert_eq!(result.is_ok(), case.accepted, "{}", case.name);
        if let Ok(message) = result {
            assert_eq!(
                serde_json::to_value(&message).unwrap(),
                case.normalized.unwrap(),
                "{}",
                case.name
            );
            let frame = encode_frame(&message, MAX_MESSAGE_BYTES).unwrap();
            assert_eq!(
                u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize,
                frame.len() - 4
            );
            assert_eq!(
                decode(&read_frame(&mut frame.as_slice(), MAX_MESSAGE_BYTES).unwrap()).unwrap(),
                message
            );
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
