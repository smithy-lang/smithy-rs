/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::collections::HashMap;

use aws_config::{provider_config::ProviderConfig, Region};
use aws_runtime::user_agent::test_util::{
    assert_ua_contains_metric_values, get_sdk_metric_str_from_request,
};
use aws_sdk_s3::{
    config::{
        http::{HttpRequest, HttpResponse},
        HttpClient, RuntimeComponents,
    },
    Client, Config,
};
use aws_smithy_http_client::test_util::capture_request;
use aws_smithy_runtime_api::client::http::{
    HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_types::body::SdkBody;
use aws_types::os_shim_internal::{Env, Fs};

#[tokio::test]
async fn profile_and_sso_ua_features() {
    let (http_client, request) = capture_request(None);

    #[derive(Debug)]
    struct ClientInner {
        expected_token: &'static str,
    }
    impl HttpConnector for ClientInner {
        fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
            assert_eq!(
                self.expected_token,
                request.headers().get("x-amz-sso_bearer_token").unwrap()
            );
            HttpConnectorFuture::ready(Ok(HttpResponse::new(
                    200.try_into().unwrap(),
                    SdkBody::from("{\"roleCredentials\":{\"accessKeyId\":\"ASIARTESTID\",\"secretAccessKey\":\"TESTSECRETKEY\",\"sessionToken\":\"TESTSESSIONTOKEN\",\"expiration\": 1651516560000}}"),
                )))
        }
    }
    #[derive(Debug)]
    struct CredsClient {
        inner: SharedHttpConnector,
    }
    impl CredsClient {
        fn new(expected_token: &'static str) -> Self {
            Self {
                inner: SharedHttpConnector::new(ClientInner { expected_token }),
            }
        }
    }
    impl HttpClient for CredsClient {
        fn http_connector(
            &self,
            _settings: &HttpConnectorSettings,
            _components: &RuntimeComponents,
        ) -> SharedHttpConnector {
            self.inner.clone()
        }
    }

    let fs = Fs::from_map({
        let mut map = HashMap::new();
        map.insert(
            "/home/.aws/config".to_string(),
            br#"
[profile default]
sso_session = dev
sso_account_id = 012345678901
sso_role_name = SampleRole
region = us-east-1

[sso-session dev]
sso_region = us-east-1
sso_start_url = https://d-abc123.awsapps.com/start
                "#
            .to_vec(),
        );
        map.insert(
            "/home/.aws/sso/cache/34c6fceca75e456f25e7e99531e2425c6c1de443.json".to_string(),
            br#"
                {
                    "accessToken": "secret-access-token",
                    "expiresAt": "2199-11-14T04:05:45Z",
                    "refreshToken": "secret-refresh-token",
                    "clientId": "ABCDEFG323242423121312312312312312",
                    "clientSecret": "ABCDE123",
                    "registrationExpiresAt": "2199-03-06T19:53:17Z",
                    "region": "us-east-1",
                    "startUrl": "https://d-abc123.awsapps.com/start"
                }
                "#
            .to_vec(),
        );
        map
    });
    let provider_config = ProviderConfig::empty()
        .with_fs(fs.clone())
        .with_env(Env::from_slice(&[("HOME", "/home")]))
        .with_http_client(CredsClient::new("secret-access-token"));
    let provider = aws_config::profile::credentials::Builder::default()
        .configure(&provider_config)
        .build();

    let config = Config::builder()
        .with_test_defaults()
        .region(Region::from_static("fake"))
        .http_client(http_client.clone())
        .credentials_provider(provider)
        .build();

    let client = Client::from_conf(config);

    let _ = client
        .head_bucket()
        .bucket("fake")
        .send()
        .await
        .expect("XXXXXXXXXXX");

    let request = request.expect_request();
    let ua = get_sdk_metric_str_from_request(&request);
    assert_ua_contains_metric_values(ua, &["n", "s"]);
}
