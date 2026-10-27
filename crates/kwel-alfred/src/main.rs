use kwel_alfred::{Output, PROFILE_DIAGNOSTIC_VARIABLE, WAIT_VARIABLE};
use kwel_client::{Client, Operation};
#[cfg(target_os = "macos")]
use kwel_native_host::install::{self, Layout};
use kwel_native_host::paths::RuntimePaths;
use kwel_protocol::{Code, Error, LocalRequest, Result};
#[tokio::main(worker_threads = 2)]
async fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|value| value == "open") {
        if open(&args).await.is_err() {
            println!("Could not open this result. Search again.");
            std::process::exit(1);
        }
        return;
    }
    let output = search(&args).await;
    match serde_json::to_string(&output) {
        Ok(json) => println!("{json}"),
        Err(_) => std::process::exit(1),
    }
}
async fn search(args: &[String]) -> Output {
    let profile = args
        .windows(2)
        .find(|pair| pair[0] == "--profile-id")
        .map(|pair| pair[1].trim().to_lowercase())
        .unwrap_or_default();
    if !kwel_protocol::id(&profile) {
        return kwel_alfred::configuration_required();
    }
    let operation = match kwel_client::parse_args(args, "alfred") {
        Ok(Operation::Request(request @ LocalRequest::Search { .. })) => request,
        _ => return kwel_alfred::notice("Enter a search query of up to 200 characters."),
    };
    let paths = match RuntimePaths::current() {
        Ok(paths) => paths,
        Err(error) => return kwel_alfred::notice(error.0.message()),
    };
    #[cfg(target_os = "macos")]
    {
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                return kwel_alfred::notice(Error::from(error).0.message());
            }
        };
        let source = match executable.parent() {
            Some(source) => source,
            None => return kwel_alfred::notice(Code::UnsafePath.message()),
        };
        let layout = match Layout::current() {
            Ok(layout) => layout,
            Err(error) => return kwel_alfred::notice(error.0.message()),
        };
        if let Err(error) = install::install(source, &layout, &paths, true) {
            return kwel_alfred::notice(error.0.message());
        }
    }
    let client = Client { paths };
    match client.request(operation).await {
        Ok(response) => kwel_alfred::response(response, &profile),
        Err(Error(code)) if kwel_alfred::can_diagnose_profile_mismatch(code) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            wait_or_diagnose(
                &client,
                code,
                &profile,
                &std::env::var(WAIT_VARIABLE).unwrap_or_default(),
                &std::env::var(PROFILE_DIAGNOSTIC_VARIABLE).unwrap_or_default(),
                now,
            )
            .await
        }
        Err(error) => kwel_alfred::notice(error.0.message()),
    }
}
async fn wait_or_diagnose(
    client: &Client,
    code: Code,
    configured_profile: &str,
    previous_deadline: &str,
    previous_attempt: &str,
    now: u64,
) -> Output {
    let attempt_diagnostic =
        kwel_alfred::profile_diagnostic_due(previous_deadline, previous_attempt, now);
    let output = if attempt_diagnostic {
        match tokio::time::timeout(std::time::Duration::from_secs(5), client.profiles()).await {
            Ok(Ok(profiles)) => kwel_alfred::profile_mismatch(configured_profile, &profiles)
                .unwrap_or_else(|| kwel_alfred::wait(code, previous_deadline, now)),
            _ => kwel_alfred::wait(code, previous_deadline, now),
        }
    } else {
        kwel_alfred::wait(code, previous_deadline, now)
    };
    let attempt = if attempt_diagnostic {
        now.to_string()
    } else {
        previous_attempt.to_owned()
    };
    if attempt.is_empty() {
        output
    } else {
        let mut output = output;
        output
            .variables
            .insert(PROFILE_DIAGNOSTIC_VARIABLE.into(), attempt);
        output
    }
}
async fn open(args: &[String]) -> Result<()> {
    let request = match kwel_client::parse_args(args, "alfred")? {
        Operation::Request(request @ LocalRequest::Open { .. }) => request,
        _ => return Err(Error(Code::InvalidRequest)),
    };
    kwel_alfred::open_acknowledged(Client::current()?.request(request), activate_chrome).await
}
fn activate_chrome() -> Result<()> {
    #[cfg(target_os = "macos")]
    if !std::process::Command::new("/usr/bin/open")
        .args(["-b", "com.google.Chrome"])
        .status()?
        .success()
    {
        return Err(Error(Code::OpenFailed));
    }
    Ok(())
}
