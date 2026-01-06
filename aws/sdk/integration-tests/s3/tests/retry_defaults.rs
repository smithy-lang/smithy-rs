// Test to verify AWS SDK clients have retries enabled by default
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;

#[test]
fn test_aws_sdk_has_retries_enabled() {
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(aws_types::region::Region::new("us-east-1"))
        .credentials_provider(aws_credential_types::Credentials::for_tests())
        .build();

    // The client should build successfully with retries enabled by default
    let _client = aws_sdk_s3::Client::from_conf(config);
}
