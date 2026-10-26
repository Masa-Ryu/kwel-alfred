use kwel_protocol::*;
use serde_json::{Value, json};

#[test]
fn native_validators_reject_client_routing_and_open_without_a_profile_id() {
    let mut search = decode(br#"{"v":1,"type":"search","requestId":"r","query":""}"#).unwrap();
    assert_eq!(search.limit, 20);
    search.search().unwrap();
    search.profile_id = "00000000-0000-4000-8000-00000000000a".into();
    assert!(search.search().is_err());
    let mut open = decode(
        br#"{"v":1,"type":"open-page","requestId":"r","openRef":"opaque-expired-reference"}"#,
    )
    .unwrap();
    open.open().unwrap();
    open.profile_id = "00000000-0000-4000-8000-00000000000a".into();
    assert!(open.open().is_err());
}

#[test]
fn local_client_still_requires_a_profile_and_uuid_open_reference() {
    let profile = "00000000-0000-4000-8000-00000000000a";
    let reference = "00000000-0000-4000-8000-000000000101";
    for value in [
        json!({"v":1,"type":"search","requestId":"r","profileId":profile,"query":"","limit":20}),
        json!({"v":1,"type":"open-page","requestId":"r","profileId":profile,"openRef":reference}),
    ] {
        let request: LocalRequest = serde_json::from_value(value.clone()).unwrap();
        request.validate().unwrap();
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("profileId");
        assert!(serde_json::from_value::<LocalRequest>(missing).is_err());
        let mut invalid = value;
        invalid["profileId"] = json!("not-a-profile");
        assert!(
            serde_json::from_value::<LocalRequest>(invalid)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let invalid: LocalRequest = serde_json::from_value(json!({"v":1,"type":"open-page","requestId":"r","profileId":profile,"openRef":"https://example.com"})).unwrap();
    assert!(invalid.validate().is_err());
}

#[test]
fn empty_search_is_explicit_and_other_messages_do_not_acquire_query() {
    let message = Message {
        request_id: "r".into(),
        limit: 20,
        ..Message::new("search")
    };
    message.search().unwrap();
    let frame = encode_frame(&message, MAX_MESSAGE_BYTES).unwrap();
    let value: Value = serde_json::from_slice(&frame[4..]).unwrap();
    assert_eq!(
        value,
        json!({"v":1,"type":"search","requestId":"r","query":"","limit":20})
    );
    for kind in ["hello-ack", "ping", "pong"] {
        assert_eq!(
            serde_json::to_value(Message::new(kind)).unwrap(),
            json!({"v":1,"type":kind})
        );
    }
    assert!(decode(br#"{"v":1,"type":"search","requestId":"r","limit":20}"#).is_err());
}

#[test]
fn null_request_id_is_only_an_uncorrelated_native_error() {
    for code in [
        "BAD_REQUEST",
        "INCOMPATIBLE_VERSION",
        "NOT_READY",
        "SEARCH_FAILED",
        "RESULT_EXPIRED",
        "OPEN_FAILED",
    ] {
        let error =
            json!({"v":1,"type":"error","requestId":null,"code":code,"message":"Safe error."});
        let message = decode(&serde_json::to_vec(&error).unwrap()).unwrap();
        assert!(message.request_id.is_empty());
        assert!(message.response("r", false, 20).is_err());
        assert!(message.response("", false, 20).is_err());
        assert_eq!(serde_json::to_value(message).unwrap(), error);
    }
    for kind in ["search-result", "open-result", "pong"] {
        let mut value = json!({"v":1,"type":kind,"requestId":null});
        if kind == "search-result" {
            value["items"] = json!([]);
            value["hasMore"] = json!(false);
        }
        if kind == "open-result" {
            value["ok"] = json!(true);
        }
        assert!(decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let legacy = decode(br#"{"v":1,"type":"error","code":"BAD_REQUEST"}"#).unwrap();
    assert_eq!(
        serde_json::to_value(&legacy).unwrap()["requestId"],
        Value::Null
    );
    assert!(legacy.response("r", false, 20).is_err());
    assert!(decode(br#"{"v":1,"type":"error","requestId":"","code":"BAD_REQUEST"}"#).is_err());
    let response =
        decode(br#"{"v":1,"type":"search-result","requestId":"one","items":[],"hasMore":false}"#)
            .unwrap();
    assert!(response.response("two", false, 20).is_err());
    response.response("one", false, 20).unwrap();
}

#[test]
fn utf16_boundaries_include_bmp_astral_and_mixed_sequences() {
    for (query, accepted) in [
        (String::new(), true),
        ("あ".repeat(200), true),
        ("あ".repeat(201), false),
        ("😀".repeat(100), true),
        ("😀".repeat(101), false),
        (format!("{}😀", "あ".repeat(198)), true),
        (format!("{}😀", "あ".repeat(199)), false),
        ("\u{10000}".repeat(100), true),
        ("\u{10ffff}".repeat(101), false),
    ] {
        let value = json!({"v":1,"type":"search","requestId":"r","query":query,"limit":20});
        assert_eq!(
            decode(&serde_json::to_vec(&value).unwrap()).is_ok(),
            accepted,
            "{} UTF-16 units",
            units(&query)
        );
    }
    let pair =
        decode(br#"{"v":1,"type":"search","requestId":"r","query":"\ud83d\ude00","limit":20}"#)
            .unwrap();
    assert_eq!(units(&pair.query), 2);
    for surrogate in [r#""\ud83d""#, r#""\ude00""#] {
        assert!(
            decode(
                format!(
                    r#"{{"v":1,"type":"search","requestId":"r","query":{surrogate},"limit":20}}"#
                )
                .as_bytes()
            )
            .is_err()
        );
    }
}

#[test]
fn current_kwel_native_only_fixture_is_accepted_without_local_routing() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../docs/fixtures/native-messaging-v1.json"
    ))
    .unwrap();
    let mut count = 0;
    for case in fixture["hostMessages"].as_array().unwrap() {
        let value = &case["message"];
        assert_eq!(
            decode(&serde_json::to_vec(value).unwrap()).is_ok(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        count += 1;
    }
    for case in fixture["hello"].as_array().unwrap() {
        let mut value = case["identity"].clone();
        value["v"] = json!(1);
        value["type"] = json!("hello");
        assert_eq!(
            decode(&serde_json::to_vec(&value).unwrap()).is_ok(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        count += 1;
    }
    for case in fixture["searchResults"].as_array().unwrap() {
        let value = &case["expected"];
        let message = decode(&serde_json::to_vec(value).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(message).unwrap(),
            *value,
            "{}",
            case["name"]
        );
        count += 1;
    }
    for case in fixture["errors"].as_array().unwrap() {
        let message = decode(&serde_json::to_vec(case).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(message).unwrap(), *case);
        count += 1;
    }
    for value in fixture["responses"].as_object().unwrap().values() {
        assert_eq!(
            serde_json::to_value(decode(&serde_json::to_vec(value).unwrap()).unwrap()).unwrap(),
            *value
        );
        count += 1;
    }
    assert_eq!(count, 65);
}
