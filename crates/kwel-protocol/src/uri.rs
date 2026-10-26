//! HTTP(S) result validation. Parsed URLs are never substituted for input data.
pub fn http(value: &str) -> bool {
    // WHATWG parsers repair these inputs; the wire must reject ambiguous data.
    if value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return false;
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let (without_fragment, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (hierarchy, _) = without_fragment
        .split_once('?')
        .unwrap_or((without_fragment, ""));
    let authority = hierarchy.split('/').next().unwrap_or("");
    let host = if let Some((userinfo, host)) = authority.rsplit_once('@') {
        if !userinfo
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:~!$&'()*+,;=%@".contains(&byte))
        {
            return false;
        }
        host
    } else {
        authority
    };
    // Do not reinterpret an encoded hostname as a different authority.
    if host.is_empty()
        || host.contains('%')
        || !valid_escapes(hierarchy)
        || !valid_escapes(fragment)
    {
        return false;
    }
    url::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.has_host())
}
fn valid_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.iter().enumerate().all(|(index, byte)| {
        *byte != b'%'
            || bytes
                .get(index + 1..index + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
    })
}
