use kwel_protocol::{Code, Item, LocalResponse};
use serde::Serialize;
use std::collections::BTreeMap;
/// Chrome activation is permitted only after the selected profile acknowledges
/// its opaque result reference. Failure produces the Workflow notification.
pub async fn open_acknowledged(
    response: impl std::future::Future<Output = kwel_protocol::Result<LocalResponse>>,
    activate: impl FnOnce() -> kwel_protocol::Result<()>,
) -> kwel_protocol::Result<()> {
    if !matches!(response.await?, LocalResponse::Open { ok: true, .. }) {
        return Err(kwel_protocol::Error(Code::OpenFailed));
    }
    activate()
}
pub const WAIT_VARIABLE: &str = "KWEL_CONNECTION_DEADLINE";
pub const WAIT_MILLIS: u64 = 45_000;
#[derive(Debug, Serialize)]
pub struct Output {
    pub skipknowledge: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerun: Option<f32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
    pub items: Vec<Row>,
}
#[derive(Debug, Serialize)]
pub struct Row {
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub arg: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
}
pub fn notice(title: &str) -> Output {
    Output {
        skipknowledge: true,
        rerun: None,
        variables: BTreeMap::from([(WAIT_VARIABLE.into(), String::new())]),
        items: vec![Row {
            title: title.into(),
            subtitle: String::new(),
            arg: String::new(),
            valid: false,
            variables: BTreeMap::new(),
        }],
    }
}
pub fn configuration_required() -> Output {
    let mut output = notice("Configure the Kwel Profile ID");
    output.items[0].subtitle =
        "Paste the Profile ID from Kwel Settings → Local Connection into Workflow Configuration."
            .into();
    output
}
pub fn results(items: Vec<Item>, has_more: bool, profile: &str) -> Output {
    if items.is_empty() {
        return notice("No matching Kwel pages");
    }
    let count = items.len();
    let mut output = notice("");
    output.items = items
        .into_iter()
        .map(|item| {
            let domain = if item.domain.trim().is_empty() {
                url::Url::parse(&item.url)
                    .ok()
                    .and_then(|url| url.host_str().map(String::from))
                    .unwrap_or_default()
            } else {
                item.domain
            };
            Row {
                title: item.title,
                subtitle: domain,
                arg: item.open_ref,
                valid: true,
                variables: BTreeMap::from([("KWEL_RESULT_PROFILE_ID".into(), profile.into())]),
            }
        })
        .collect();
    if has_more {
        output.items.push(Row {
            title: format!("Showing the first {count} results. Refine your search for more."),
            subtitle: String::new(),
            arg: String::new(),
            valid: false,
            variables: BTreeMap::new(),
        });
    }
    output
}
pub fn wait(code: Code, previous: &str, now: u64) -> Output {
    let maximum = now.saturating_add(WAIT_MILLIS);
    let deadline = previous
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0 && *value <= maximum)
        .unwrap_or(maximum);
    if now >= deadline {
        return notice(code.message());
    }
    let mut output = notice("Connecting to Kwel…");
    output.items[0].subtitle = "Keep Chrome open. Search results will appear automatically.".into();
    output.rerun = Some(1.0);
    output
        .variables
        .insert(WAIT_VARIABLE.into(), deadline.to_string());
    output
}
pub fn response(response: LocalResponse, profile: &str) -> Output {
    match response {
        LocalResponse::Search {
            items, has_more, ..
        } => results(items, has_more, profile),
        LocalResponse::Error { code, .. } => notice(code.message()),
        _ => notice(Code::SearchFailed.message()),
    }
}
