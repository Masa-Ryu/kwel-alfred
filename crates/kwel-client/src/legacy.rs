// Private read-only Go local-socket adapter. Never accepted on the Native wire.
use super::*;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Response {
    ProfileInfo {
        v: u32,
        profile_id: String,
        #[serde(default)]
        profile_label: String,
        #[serde(default)]
        message: String,
    },
    SearchResult {
        v: u32,
        request_id: String,
        // Go omitted empty items. This exception belongs only to this adapter.
        #[serde(default)]
        items: Vec<kwel_protocol::Item>,
        has_more: bool,
    },
    OpenResult {
        v: u32,
        request_id: String,
        ok: bool,
    },
    Error {
        v: u32,
        #[serde(default)]
        request_id: Option<String>,
        code: Code,
        #[serde(default, rename = "message")]
        _message: String,
    },
}
fn convert(bytes: &[u8], request: &LocalRequest) -> Result<LocalResponse> {
    let local = request.message();
    let response: Response = serde_json::from_slice(bytes)?;
    let v = match &response {
        Response::ProfileInfo { v, .. }
        | Response::SearchResult { v, .. }
        | Response::OpenResult { v, .. }
        | Response::Error { v, .. } => *v,
    };
    if v != VERSION {
        return Err(Error(Code::IncompatibleVersion));
    }
    match (request, response) {
        (
            LocalRequest::Probe { .. },
            Response::ProfileInfo {
                profile_id,
                profile_label,
                message,
                ..
            },
        ) if profile_id == local.profile_id => Ok(LocalResponse::profile(
            &local.request_id,
            &Profile {
                profile_id,
                profile_label,
                host_version: if message.is_empty() {
                    "legacy-v1".into()
                } else {
                    message
                },
            },
        )),
        (
            LocalRequest::Search { .. },
            Response::SearchResult {
                request_id,
                items,
                has_more,
                ..
            },
        ) => {
            Message {
                request_id: request_id.clone(),
                items: items.clone(),
                has_more: Some(has_more),
                ..Message::new("search-result")
            }
            .response(&local.request_id, false, local.limit)?;
            Ok(LocalResponse::Search {
                v: VERSION,
                request_id,
                items,
                has_more,
            })
        }
        (LocalRequest::Open { .. }, Response::OpenResult { request_id, ok, .. }) => {
            Message {
                request_id: request_id.clone(),
                ok,
                ..Message::new("open-result")
            }
            .response(&local.request_id, true, 0)?;
            Ok(LocalResponse::Open {
                v: VERSION,
                request_id,
                ok,
            })
        }
        (
            _,
            Response::Error {
                request_id, code, ..
            },
        ) if code.native()
            && (request_id.as_deref() == Some(&local.request_id)
                || (matches!(request, LocalRequest::Probe { .. }) && request_id.is_none())) =>
        {
            Err(Error(code))
        }
        _ => Err(Error(Code::InvalidRequest)),
    }
}
pub(super) async fn exchange(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin + ?Sized),
    request: &LocalRequest,
) -> Result<LocalResponse> {
    request.validate()?;
    tokio::time::timeout(Duration::from_secs(15), async {
        // The old socket uses routed Local requests, not strict Native Message.
        ipc::write(stream, request).await?;
        let bytes = ipc::read(stream, MAX_MESSAGE_BYTES).await?;
        convert(&bytes, request)
    })
    .await
    .map_err(|_| Error(Code::Timeout))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_exceptions_do_not_change_native_or_allow_wrong_correlations() {
        let profile = "00000000-0000-4000-8000-00000000000a";
        let request = LocalRequest::Search {
            v: 1,
            request_id: "r".into(),
            profile_id: profile.into(),
            query: "".into(),
            limit: 20,
        };
        let empty = json!({"v":1,"type":"search-result","requestId":"r","hasMore":false});
        let bytes = serde_json::to_vec(&empty).unwrap();
        assert!(convert(&bytes, &request).is_ok());
        assert!(kwel_protocol::decode(&bytes).is_err());
        assert!(kwel_protocol::encode_frame(&request.message(), MAX_MESSAGE_BYTES).is_err());
        for response in [
            json!({"v":1,"type":"search-result","requestId":"other","hasMore":false}),
            json!({"v":1,"type":"search-result","requestId":null,"hasMore":false}),
            json!({"v":1,"type":"search-result","requestId":"r"}),
            json!({"v":2,"type":"search-result","requestId":"r","hasMore":false}),
            json!({"v":1,"type":"search-result","requestId":"r","hasMore":false,"profileId":profile}),
            json!({"v":1,"type":"error","requestId":null,"code":"BAD_REQUEST"}),
            json!({"v":1,"type":"error","requestId":"other","code":"BAD_REQUEST"}),
            json!({"v":1,"type":"profile-ack"}),
            json!({"v":1,"type":"uninstall-ack"}),
        ] {
            assert!(convert(&serde_json::to_vec(&response).unwrap(), &request).is_err());
        }
        let probe = LocalRequest::Probe {
            v: 1,
            request_id: "probe".into(),
            profile_id: profile.into(),
        };
        assert!(
            convert(
                br#"{"v":1,"type":"profile-info","profileId":"wrong"}"#,
                &probe
            )
            .is_err()
        );
        assert!(matches!(
            convert(
                br#"{"v":1,"type":"error","code":"PROFILE_CONFLICT"}"#,
                &probe
            ),
            Err(Error(Code::ProfileConflict))
        ));
    }
    #[tokio::test]
    async fn go_probe_and_omitted_empty_results_use_private_legacy_shapes() {
        let profile = "00000000-0000-4000-8000-00000000000a";
        for (request, response) in [
            (
                LocalRequest::Probe {
                    v: 1,
                    request_id: "local".into(),
                    profile_id: profile.into(),
                },
                json!({"v":1,"type":"profile-info","profileId":profile,"message":"1.00"}),
            ),
            (
                LocalRequest::Search {
                    v: 1,
                    request_id: "local".into(),
                    profile_id: profile.into(),
                    query: "".into(),
                    limit: 20,
                },
                json!({"v":1,"type":"search-result","requestId":"local","hasMore":false}),
            ),
            (
                LocalRequest::Open {
                    v: 1,
                    request_id: "local".into(),
                    profile_id: profile.into(),
                    open_ref: "00000000-0000-4000-8000-000000000101".into(),
                },
                json!({"v":1,"type":"open-result","requestId":"local","ok":true}),
            ),
        ] {
            let (mut client, mut server) = tokio::io::duplex(4096);
            let expected = serde_json::to_value(&request).unwrap();
            let peer = tokio::spawn(async move {
                let bytes = ipc::read(&mut server, MAX_MESSAGE_BYTES).await.unwrap();
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                    expected
                );
                ipc::write(&mut server, &response).await.unwrap();
            });
            let result = exchange(&mut client, &request).await;
            assert!(
                result.is_ok(),
                "Private legacy exchange must remain usable: {result:?}"
            );
            peer.await.unwrap();
        }
    }
}
