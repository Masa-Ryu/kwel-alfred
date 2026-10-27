use kwel_alfred::{Output, WAIT_VARIABLE};
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
    let result = async {
        let paths = RuntimePaths::current()?;
        #[cfg(target_os = "macos")]
        {
            let executable = std::env::current_exe()?;
            let source = executable.parent().ok_or(Error(Code::UnsafePath))?;
            install::install(source, &Layout::current()?, &paths, true)?;
        }
        Client { paths }.request(operation).await
    }
    .await;
    match result {
        Ok(response) => kwel_alfred::response(response, &profile),
        Err(Error(code @ (Code::Disconnected | Code::HostNotFound))) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            kwel_alfred::wait(code, &std::env::var(WAIT_VARIABLE).unwrap_or_default(), now)
        }
        Err(error) => kwel_alfred::notice(error.0.message()),
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
