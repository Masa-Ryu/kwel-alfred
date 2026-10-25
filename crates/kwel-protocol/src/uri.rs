//! Preserve the Go v1 HTTP URL acceptance boundary without WHATWG rewriting.
//! These URLs are result data only: opening always uses an opaque openRef.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Escapes {
    Component,
    Host,
    Zone,
}
fn percent(value: &str, mode: Escapes) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut offset = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte == b'%' {
            let pair = bytes.get(offset + 1..offset + 3)?;
            let high = (pair[0] as char).to_digit(16)? as u8;
            let low = (pair[1] as char).to_digit(16)? as u8;
            let byte = high * 16 + low;
            if (mode == Escapes::Host && byte < 128 && byte != b'%')
                || (mode == Escapes::Zone && byte != b'%' && byte != b' ' && !host_byte(byte))
            {
                return None;
            }
            decoded.push(byte);
            offset += 3;
        } else {
            if mode != Escapes::Component && !host_byte(byte) {
                return None;
            }
            decoded.push(byte);
            offset += 1;
        }
    }
    Some(decoded)
}
fn host_byte(byte: u8) -> bool {
    byte >= 128 || byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:[]<>\"".contains(&byte)
}
fn port(value: &str) -> bool {
    value.is_empty()
        || value
            .strip_prefix(':')
            .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
}
fn host(value: &str) -> bool {
    match value.rfind('[') {
        Some(0) => {
            let Some(end) = value.rfind(']') else {
                return false;
            };
            if !port(&value[end + 1..]) {
                return false;
            }
            let literal = &value[1..end];
            let address = if let Some(zone) = literal.find("%25") {
                if !percent(&literal[zone..], Escapes::Zone).is_some_and(|value| value.len() > 1) {
                    return false;
                }
                &literal[..zone]
            } else {
                literal
            };
            percent(address, Escapes::Host)
                .and_then(|value| String::from_utf8(value).ok())
                .is_some_and(|value| value.parse::<std::net::Ipv6Addr>().is_ok())
        }
        Some(_) => false,
        None => {
            let hostname = if let Some((hostname, number)) = value.rsplit_once(':') {
                if !number.bytes().all(|byte| byte.is_ascii_digit()) {
                    return false;
                }
                hostname
            } else {
                value
            };
            !hostname.is_empty() && percent(value, Escapes::Host).is_some()
        }
    }
}
pub fn http(value: &str) -> bool {
    if value.bytes().any(|byte| byte < 32 || byte == 127) {
        return false;
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    if percent(fragment, Escapes::Component).is_none() {
        return false;
    }
    // Go leaves RawQuery uninterpreted, but validates path/fragment escapes.
    let (rest, _) = rest.split_once('?').unwrap_or((rest, ""));
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if percent(path, Escapes::Component).is_none() {
        return false;
    }
    let hostname = if let Some((userinfo, hostname)) = authority.rsplit_once('@') {
        if !userinfo
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:~!$&'()*+,;=%@".contains(&byte))
            || percent(userinfo, Escapes::Component).is_none()
        {
            return false;
        }
        hostname
    } else {
        authority
    };
    host(hostname)
}
