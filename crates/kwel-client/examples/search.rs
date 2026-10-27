use kwel_client::Client;
use kwel_protocol::{LocalRequest, VERSION};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let profile_id = std::env::var("KWEL_PROFILE_ID")?;
    let query = std::env::args().nth(1).unwrap_or_else(|| "Kyoto".into());
    let response = Client::current()?
        .request(LocalRequest::Search {
            v: VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            profile_id,
            query,
            limit: 20,
        })
        .await?;
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}
