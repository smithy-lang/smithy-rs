use kms::operation::GenerateRandom;
use kms::Region;
use env_logger::Env;

#[tokio::main]
async fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("trace"));
    let config = kms::Config::builder()
        // region can also be loaded from AWS_DEFAULT_REGION, just remove this line.
        .region(Region::from("us-east-1"))
        // creds loaded from environment variables, or they can be hard coded.
        // Other credential providers not currently supported
        .build();
    let client = aws_hyper::Client::https();
    let data = client
        .call(GenerateRandom::builder().number_of_bytes(64).build(&config))
        .await
        .expect("failed to generate random data");
    println!("{:?}", data);
}
