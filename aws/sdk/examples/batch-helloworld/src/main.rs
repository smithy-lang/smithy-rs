#[tokio::main]
async fn main() -> Result<(), batch::Error> {
    tracing_subscriber::fmt::init();

    let client = batch::Client::from_env();
    let compute_envs = client
        .describe_compute_environments()
        .max_results(25)
        .send()
        .await?;

    for env in compute_envs.compute_environments.unwrap_or_default() {
        let arn = env.compute_environment_arn.as_deref().unwrap_or_default();
        let name = env.compute_environment_name.as_deref().unwrap_or_default();

        println!(
            "Compute Environment Name : {}, Compute Environment ARN : {}",
            name, arn
        );
    }

    println!("Hello, world!");

    Ok(())
}
