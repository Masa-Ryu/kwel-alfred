use kwel_protocol::*;
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
