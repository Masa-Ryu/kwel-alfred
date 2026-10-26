use kwel_protocol::*;
use serde_json::{Value, json};

fn schema(source: &str) -> jsonschema::Validator {
    let value: Value = serde_json::from_str(source).unwrap();
    assert_eq!(
        value["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    jsonschema::draft202012::meta::validate(&value).unwrap();
    jsonschema::draft202012::new(&value).unwrap()
}

const NATIVE_SCHEMA: &str = include_str!("../../../docs/schemas/native-messaging-v1.schema.json");
const LOCAL_SCHEMA: &str = include_str!("../../../docs/schemas/local-client-v1.schema.json");

#[test]
fn specification_examples_validate_against_the_corresponding_schema() {
    let mut item_schema: Value = serde_json::from_str(NATIVE_SCHEMA).unwrap();
    item_schema.as_object_mut().unwrap().remove("oneOf");
    item_schema["$ref"] = json!("#/$defs/item");
    let item = jsonschema::draft202012::new(&item_schema).unwrap();
    for (document, source, expected) in [
        (
            include_str!("../../../docs/native-messaging-v1.md"),
            NATIVE_SCHEMA,
            10,
        ),
        (
            include_str!("../../../docs/local-client-protocol-v1.md"),
            LOCAL_SCHEMA,
            7,
        ),
    ] {
        let validator = schema(source);
        // Exercise both checkout styles on every OS; fixture bytes are untouched.
        for newline in ["\n", "\r\n"] {
            let document = document.lines().collect::<Vec<_>>().join(newline);
            let mut count = 0;
            for example in document.split("```json").skip(1) {
                let value: Value =
                    serde_json::from_str(example.split("```").next().unwrap()).unwrap();
                let validator = if value.get("type").is_some() {
                    &validator
                } else {
                    &item
                };
                assert!(
                    validator.is_valid(&value),
                    "Invalid documented example: {value}"
                );
                count += 1;
            }
            assert_eq!(count, expected);
        }
    }
}

#[test]
fn schemas_close_every_message_and_require_its_fields() {
    let profile = "00000000-0000-4000-8000-00000000000a";
    let reference = "00000000-0000-4000-8000-000000000101";
    let native = [
        json!({"v":1,"type":"hello","profileId":profile}),
        json!({"v":1,"type":"hello-ack"}),
        json!({"v":1,"type":"ping"}),
        json!({"v":1,"type":"pong"}),
        json!({"v":1,"type":"search","requestId":"r","query":""}),
        json!({"v":1,"type":"search-result","requestId":"r","items":[],"hasMore":false}),
        json!({"v":1,"type":"open-page","requestId":"r","openRef":reference}),
        json!({"v":1,"type":"open-result","requestId":"r","ok":true}),
        json!({"v":1,"type":"error","code":"BAD_REQUEST"}),
    ];
    let local = [
        json!({"v":1,"type":"profile-probe","requestId":"r","profileId":profile}),
        json!({"v":1,"type":"search","requestId":"r","profileId":profile,"query":"","limit":20}),
        json!({"v":1,"type":"open-page","requestId":"r","profileId":profile,"openRef":reference}),
        json!({"v":1,"type":"profile-info","requestId":"r","profileId":profile,"profileLabel":"","hostVersion":"1.0.0"}),
        json!({"v":1,"type":"search-result","requestId":"r","items":[],"hasMore":false}),
        json!({"v":1,"type":"open-result","requestId":"r","ok":true}),
        json!({"v":1,"type":"error","requestId":"","code":"INVALID_REQUEST","message":"The request is invalid."}),
    ];
    for (source, messages) in [
        (NATIVE_SCHEMA, native.as_slice()),
        (LOCAL_SCHEMA, local.as_slice()),
    ] {
        let validator = schema(source);
        for value in messages {
            assert!(validator.is_valid(value), "{value}");
            let mut unknown = value.clone();
            unknown["extra"] = json!(true);
            assert!(!validator.is_valid(&unknown), "{unknown}");
            for field in value.as_object().unwrap().keys() {
                let mut missing = value.clone();
                missing.as_object_mut().unwrap().remove(field);
                assert!(!validator.is_valid(&missing), "missing {field}: {missing}");
            }
            let mut version = value.clone();
            version["v"] = json!(2);
            assert!(!validator.is_valid(&version));
        }
    }
}

#[test]
fn local_request_schema_matches_rust_fields_defaults_and_uuid_policy() {
    let validator = schema(LOCAL_SCHEMA);
    let profile = "00000000-0000-4000-8000-00000000000a";
    let search =
        json!({"v":1,"type":"search","requestId":"r","profileId":profile,"query":"","limit":20});
    let mut cases = vec![(search.clone(), true)];
    for (field, replacement, accepted) in [
        ("profileId", json!(format!("{profile}\n")), false),
        ("profileId", json!("not-a-profile"), false),
        ("requestId", Value::Null, false),
        ("requestId", json!(""), false),
        ("query", Value::Null, false),
        ("query", json!("あ".repeat(200)), true),
        ("query", json!("あ".repeat(201)), false),
        ("limit", json!(1), true),
        ("limit", json!(100), true),
        ("limit", json!(0), false),
        ("limit", json!(101), false),
        ("limit", json!(1.5), false),
    ] {
        let mut value = search.clone();
        value[field] = replacement;
        cases.push((value, accepted));
    }
    for field in ["query", "limit", "profileId"] {
        let mut missing = search.clone();
        missing.as_object_mut().unwrap().remove(field);
        cases.push((missing, false));
    }
    for (reference, accepted) in [
        ("00000000-0000-4000-8000-000000000101", true),
        ("https://example.com", false),
        ("opaque", false),
        ("00000000-0000-4000-8000-000000000101\n", false),
    ] {
        cases.push((json!({"v":1,"type":"open-page","requestId":"r","profileId":profile,"openRef":reference}), accepted));
    }
    for (value, accepted) in cases {
        assert_eq!(validator.is_valid(&value), accepted, "Schema: {value}");
        assert_eq!(
            serde_json::from_value::<LocalRequest>(value.clone())
                .is_ok_and(|request| request.validate().is_ok()),
            accepted,
            "Rust: {value}"
        );
    }
}

#[test]
fn schemas_match_result_items_and_keep_native_and_local_errors_separate() {
    let native = schema(NATIVE_SCHEMA);
    let local = schema(LOCAL_SCHEMA);
    for title in ["Kyoto", "", " \t\r\n", "\u{85}\u{3000}", "\u{feff}"] {
        let result = json!({"v":1,"type":"search-result","requestId":"r","items":[{"title":title,"url":"https://example.com/","domain":"example.com","openRef":"00000000-0000-4000-8000-000000000101"}],"hasMore":false});
        let expected = !title.trim().is_empty();
        assert_eq!(native.is_valid(&result), expected, "Native title {title:?}");
        assert_eq!(local.is_valid(&result), expected, "Local title {title:?}");
        assert_eq!(
            decode(&serde_json::to_vec(&result).unwrap()).is_ok(),
            expected
        );
    }
    for request_id in ["".into(), "r".into(), "r".repeat(101), "😀".repeat(51)] {
        let value =
            serde_json::to_value(LocalResponse::error(&request_id, Code::InvalidRequest)).unwrap();
        assert!(
            local.is_valid(&value),
            "Local errors echo a decoded invalid ID or use an empty ID"
        );
    }
    let uncorrelated =
        json!({"v":1,"type":"error","requestId":null,"code":"BAD_REQUEST","message":""});
    assert!(native.is_valid(&uncorrelated));
    assert!(!local.is_valid(&uncorrelated));
    let installation = serde_json::to_value(LocalResponse::error("r", Code::Busy)).unwrap();
    assert!(local.is_valid(&installation));
    assert!(!native.is_valid(&installation));
}

#[test]
fn standard_schema_annotations_do_not_replace_wire_and_session_validation() {
    let validator = schema(NATIVE_SCHEMA);
    let omitted = json!({"v":1,"type":"search","requestId":"r","query":""});
    assert!(validator.is_valid(&omitted));
    assert!(
        omitted.get("limit").is_none(),
        "default is an annotation, never an edit"
    );
    assert_eq!(
        decode(&serde_json::to_vec(&omitted).unwrap())
            .unwrap()
            .limit,
        20
    );
    let astral = json!({"v":1,"type":"search","requestId":"r","query":"😀".repeat(101)});
    assert!(
        validator.is_valid(&astral),
        "standard validators ignore x-utf16-maxLength"
    );
    assert!(decode(&serde_json::to_vec(&astral).unwrap()).is_err());
    for wire in [
        r#"{"v":1.0,"type":"ping"}"#,
        r#"{"v":1,"v":1,"type":"ping"}"#,
    ] {
        let value: Value = serde_json::from_str(wire).unwrap();
        assert!(
            validator.is_valid(&value),
            "Schema validates values, not integer spellings or duplicate keys"
        );
        assert!(decode(wire.as_bytes()).is_err());
    }
    let response =
        json!({"v":1,"type":"search-result","requestId":"other","items":[],"hasMore":false});
    assert!(validator.is_valid(&response));
    assert!(
        decode(&serde_json::to_vec(&response).unwrap())
            .unwrap()
            .response("pending", false, 20)
            .is_err()
    );
    let unsafe_url = json!({"v":1,"type":"search-result","requestId":"r","items":[{"title":"Kyoto","url":"https://example.com:65536/","domain":"example.com","openRef":"00000000-0000-4000-8000-000000000101"}],"hasMore":false});
    assert!(
        validator.is_valid(&unsafe_url),
        "HTTP prefix does not validate an authority"
    );
    assert!(decode(&serde_json::to_vec(&unsafe_url).unwrap()).is_err());
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
    let native = schema(NATIVE_SCHEMA);
    let local = schema(LOCAL_SCHEMA);
    for kind in ["uninstall", "doctor", "profile-claim", "profile-release"] {
        let value = json!({"v":1,"type":kind,"requestId":"r","profileId":profile});
        assert!(!native.is_valid(&value));
        assert!(!local.is_valid(&value));
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
fn origins_are_exact_and_profile_ids_require_rfc_uuid_versions() {
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

#[test]
fn http_result_urls_use_standard_parsing_without_unsafe_repairs() {
    for (url, accepted) in [
        ("HTTPS://example.com/", true),
        (" https://example.com/", false),
        ("https://example.com/ ", true),
        ("https://example.com/with space", true),
        ("https://example.com/\npath", false),
        ("https://exa\tmple.com/", false),
        ("https:\\example.com/", false),
        ("https:example.com/", false),
        ("https:///example.com/", false),
        ("https:////example.com/", false),
        ("https://example.com:65536/", false),
        ("https://example.com:abc/", false),
        ("https://example.com:/", true),
        ("https://%65xample.com/", false),
        ("https://example.com/%zz", false),
        ("https://example.com/?q=%zz", true),
        ("https://example.com/#%zz", false),
        ("https://[::1]/", true),
        ("https://[fe80::1%25en0]/", false),
        ("https://user:pass@example.com/path", true),
        ("https://example.com/\\x", false),
        ("https://example.com/a\\b", false),
        ("https://example.com/?q=\\path", false),
        ("https://example.com/#\\part", false),
        ("https://%c3%a9xample.com/a", false),
        ("https://a%25b/a", false),
        ("https://{example.com}/a", false),
        ("https://example.com`/a", false),
        ("https://example.com\"/a", false),
        ("https://[bad]/", false),
        ("https://exa|mple.com/", false),
        ("https://127.1/", true),
        ("https://999.999.999.999/", false),
        ("http://a:b:80/", false),
        ("http://[::ffff:127.0.0.1]/", true),
        ("http://[::1%25]/", false),
        ("http://[v1.foo]/", false),
        ("http://]/", false),
        ("http://[bad]/", false),
        ("http://a<b>/", false),
        ("http://a%25b/", false),
        ("http://%ff/", false),
        ("http://user%ff:pass@host/", true),
        ("http://é@host/", false),
        ("http://host/%00", true),
        ("http://host\u{7f}/", false),
        ("https://example.com\\@evil.example/", false),
        ("https://example.com/%", false),
        ("https://[::1]:443/path", true),
        ("https://例え.テスト/path?q=京都#項目", true),
        ("https://example.com/path%20name#part%20one", true),
        ("https://%31%32%37.0.0.1/", false),
    ] {
        assert_eq!(http_url(url), accepted, "{url:?}");
        let result = serde_json::json!({"v":1,"type":"search-result","requestId":"r","hasMore":false,"items":[{"title":"Boundary","url":url,"domain":"","openRef":"00000000-0000-4000-8000-000000000101"}]});
        assert_eq!(
            decode(&serde_json::to_vec(&result).unwrap()).is_ok(),
            accepted,
            "result {url:?}"
        );
    }
}

#[test]
fn native_request_ids_labels_and_result_counts_keep_their_bounds() {
    for (size, accepted) in [(0, false), (100, true), (101, false)] {
        let value = serde_json::json!({"v":1,"type":"search","requestId":"r".repeat(size),"query":"","limit":20});
        assert_eq!(
            decode(&serde_json::to_vec(&value).unwrap()).is_ok(),
            accepted
        );
    }
    for (label, accepted) in [
        ("x".repeat(64), true),
        ("x".repeat(65), false),
        ("line\nbreak".into(), false),
    ] {
        let hello = serde_json::json!({"v":1,"type":"hello","profileId":"00000000-0000-4000-8000-00000000000a","profileLabel":label});
        assert_eq!(
            decode(&serde_json::to_vec(&hello).unwrap()).is_ok(),
            accepted
        );
    }
    let item = serde_json::json!({"title":"Boundary","url":"https://example.com/","domain":"example.com","openRef":"00000000-0000-4000-8000-000000000101"});
    for (count, accepted) in [(0, true), (100, true), (101, false)] {
        let value = serde_json::json!({"v":1,"type":"search-result","requestId":"r","items":vec![item.clone();count],"hasMore":false});
        assert_eq!(
            decode(&serde_json::to_vec(&value).unwrap()).is_ok(),
            accepted
        );
    }
}
