#[cfg(any(target_os = "macos", test))]
mod legacy;
#[cfg(target_os = "macos")]
use kwel_native_host::paths;
use kwel_native_host::{ipc, paths::RuntimePaths};
use kwel_protocol::{
    Code, Error, LocalRequest, LocalResponse, MAX_MESSAGE_BYTES, Message, Profile, Result, VERSION,
};
use std::{collections::BTreeMap, time::Duration};
#[derive(Clone)]
pub struct Client {
    pub paths: RuntimePaths,
}
impl Client {
    pub fn current() -> Result<Self> {
        Ok(Self {
            paths: RuntimePaths::current()?,
        })
    }
    pub async fn request(&self, request: LocalRequest) -> Result<LocalResponse> {
        request.validate()?;
        let message = request.message();
        let result = self.exchange(&request).await;
        #[cfg(target_os = "macos")]
        let result = match result {
            Err(Error(Code::Disconnected)) => self.legacy(&request).await,
            other => other,
        };
        let response = result?;
        if response.version() != VERSION || response.request_id() != message.request_id {
            return Err(Error(Code::InvalidRequest));
        }
        match (&request, &response) {
            (_, LocalResponse::Error { code, .. }) => return Err(Error(*code)),
            (
                LocalRequest::Probe { .. },
                LocalResponse::Profile {
                    profile_id,
                    profile_label,
                    host_version,
                    ..
                },
            ) if profile_id == &message.profile_id
                && kwel_protocol::units(profile_label) <= 64
                && !profile_label.contains(['\r', '\n'])
                && !host_version.is_empty()
                && host_version.len() <= 100 => {}
            (
                LocalRequest::Search { .. },
                LocalResponse::Search {
                    items, has_more, ..
                },
            ) => Message {
                request_id: message.request_id.clone(),
                items: items.clone(),
                has_more: Some(*has_more),
                ..Message::new("search-result")
            }
            .response(&message.request_id, false, message.limit)?,
            (LocalRequest::Open { .. }, LocalResponse::Open { ok: true, .. }) => {}
            _ => return Err(Error(Code::InvalidRequest)),
        }
        Ok(response)
    }
    async fn exchange(&self, request: &LocalRequest) -> Result<LocalResponse> {
        let message = request.message();
        let mut stream = ipc::connect(&self.paths.endpoint(&message.profile_id, false)?).await?;
        let duration = if matches!(request, LocalRequest::Probe { .. }) {
            Duration::from_secs(4)
        } else {
            Duration::from_secs(15)
        };
        tokio::time::timeout(duration, async {
            ipc::write(&mut *stream, request).await?;
            let bytes = ipc::read(&mut *stream, MAX_MESSAGE_BYTES).await?;
            Ok(serde_json::from_slice(&bytes)?)
        })
        .await
        .map_err(|_| Error(Code::Timeout))?
    }
    // Read-only adapter for an already-running Go Host. New IPC never accepts
    // its management operations; no legacy process is killed during migration.
    #[cfg(target_os = "macos")]
    async fn legacy(&self, request: &LocalRequest) -> Result<LocalResponse> {
        let message = request.message();
        let directory = paths::legacy_directory().ok_or(Error(Code::Disconnected))?;
        let mut stream =
            ipc::connect(&directory.join(format!("{}.sock", message.profile_id))).await?;
        legacy::exchange(&mut *stream, request).await
    }
    pub async fn profiles(&self) -> Result<Vec<Profile>> {
        let mut ids = self.paths.profiles()?;
        #[cfg(target_os = "macos")]
        if let Some(directory) = paths::legacy_directory()
            && let Ok(legacy) = std::fs::read_dir(directory)
        {
            for entry in legacy.flatten() {
                if let Some(id) = entry.file_name().to_string_lossy().strip_suffix(".sock")
                    && kwel_protocol::id(id)
                {
                    ids.push(id.into());
                }
            }
        }
        ids.sort();
        ids.dedup();
        ids.truncate(64);
        let mut jobs = tokio::task::JoinSet::new();
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
        for profile_id in ids {
            let client = self.clone();
            let semaphore = semaphore.clone();
            jobs.spawn(async move {
                let _permit = semaphore.acquire_owned().await.ok()?;
                match client
                    .request(LocalRequest::Probe {
                        v: VERSION,
                        request_id: uuid::Uuid::new_v4().to_string(),
                        profile_id,
                    })
                    .await
                    .ok()?
                {
                    LocalResponse::Profile {
                        profile_id,
                        profile_label,
                        host_version,
                        ..
                    } => Some(Profile {
                        profile_id,
                        profile_label,
                        host_version,
                    }),
                    _ => None,
                }
            });
        }
        let mut profiles = Vec::new();
        while let Some(result) = jobs.join_next().await {
            if let Ok(Some(profile)) = result {
                profiles.push(profile);
            }
        }
        profiles.sort_by(|a, b| a.profile_id.cmp(&b.profile_id));
        Ok(profiles)
    }
}
pub enum Operation {
    Version,
    Profiles,
    Request(LocalRequest),
}
pub fn parse_args(arguments: &[String], format: &str) -> Result<Operation> {
    let operation = arguments.first().ok_or(Error(Code::InvalidRequest))?;
    if matches!(operation.as_str(), "version" | "--version") && arguments.len() == 1 {
        return Ok(Operation::Version);
    }
    let mut values = BTreeMap::new();
    let mut args = arguments[1..].iter();
    while let Some(key) = args.next() {
        let value = args.next().ok_or(Error(Code::InvalidRequest))?;
        if !key.starts_with("--") || values.insert(key.as_str(), value.as_str()).is_some() {
            return Err(Error(Code::InvalidRequest));
        }
    }
    if values
        .remove("--format")
        .is_some_and(|value| value != format)
    {
        return Err(Error(Code::InvalidRequest));
    }
    if operation == "profiles" {
        return if values.is_empty() {
            Ok(Operation::Profiles)
        } else {
            Err(Error(Code::InvalidRequest))
        };
    }
    let profile_id = values
        .remove("--profile-id")
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let request_id = uuid::Uuid::new_v4().to_string();
    let request = match operation.as_str() {
        "profile-probe" => LocalRequest::Probe {
            v: VERSION,
            request_id,
            profile_id,
        },
        "search" => {
            let query = values.remove("--query").unwrap_or("").to_string();
            let limit = values
                .remove("--limit")
                .unwrap_or("20")
                .parse()
                .map_err(|_| Error(Code::InvalidRequest))?;
            LocalRequest::Search {
                v: VERSION,
                request_id,
                profile_id,
                query,
                limit,
            }
        }
        "open" | "open-page" => {
            let open_ref = values.remove("--ref").unwrap_or("").to_string();
            LocalRequest::Open {
                v: VERSION,
                request_id,
                profile_id,
                open_ref,
            }
        }
        _ => return Err(Error(Code::InvalidRequest)),
    };
    if !values.is_empty() {
        return Err(Error(Code::InvalidRequest));
    }
    request.validate()?;
    Ok(Operation::Request(request))
}
