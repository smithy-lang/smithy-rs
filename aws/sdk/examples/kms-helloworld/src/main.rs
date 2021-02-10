use kms::operation::GenerateRandom;
use env_logger::Env;

#[tokio::main]
async fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    let config = kms::Config::builder()
        .region("us-east-1")
        // creds loaded from environment variables, or they can be hard coded. Other credential providers not supported
        .build();
    let client = aws_hyper::Client::default().with_tracing();
    let data = client
        .call(GenerateRandom::builder().number_of_bytes(64).build(&config))
        .await
        .expect("failed to generate random");
    println!("{:?}", data);
}
