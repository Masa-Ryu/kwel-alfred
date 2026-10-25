use kwel_client::{Client, Operation};
use kwel_protocol::{LocalResponse, Result};
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        let output = LocalResponse::error("", error.0);
        if let Ok(json) = serde_json::to_string(&output) {
            println!("{json}");
        }
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match kwel_client::parse_args(&args, "json")? {
        Operation::Version => println!("{}", kwel_native_host::VERSION),
        Operation::Profiles => println!(
            "{}",
            serde_json::to_string(
                &serde_json::json!({"v":1,"profiles":Client::current()?.profiles().await?})
            )?
        ),
        Operation::Request(request) => {
            let response = Client::current()?.request(request).await?;
            println!("{}", serde_json::to_string(&response)?);
        }
    }
    Ok(())
}
