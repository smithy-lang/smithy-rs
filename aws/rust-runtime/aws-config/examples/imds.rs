use aws_config::imds::Client;
use std::error::Error;
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let imds = Client::builder().build().await?;
    let instance_id = imds.get("/latest/meta-data/instance-id").await?;
    println!("current instance id: {}", instance_id);
    Ok(())
}
