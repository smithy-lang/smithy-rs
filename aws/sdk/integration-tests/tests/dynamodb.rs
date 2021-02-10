/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth::Credentials;
use bytes::Bytes;
use dynamodb::operation::ListTables;
use smithy_http::body::SdkBody;
use std::convert::{TryFrom};
use std::process::Command;
use serde::Deserialize;
use tower::BoxError;
use reqwest::Certificate;
use std::fs;

fn start_test(id: &str) {
    let _output = Command::new("curl")
        .args(&[format!("http://crucible/clear_test")])
        .output()
        .expect("test started");
    let _output = Command::new("curl")
        .args(&[format!("http://crucible/start_test/{}", id)])
        .output()
        .expect("test started");
}

fn finish_test() -> TestResults {
    let output = Command::new("curl")
        .args(&[format!("http://crucible/check_test")])
        .output()
        .expect("test started");
    serde_json::from_slice(output.stdout.as_slice()).expect("invalid schema")
}

#[derive(Deserialize)]
struct TestResults {
    errors: Vec<String>
}

fn make_connector() -> impl tower::Service<
    http::Request<SdkBody>,
    Response = http::Response<hyper::Body>,
    Error = BoxError,
    Future = impl Send
> + Clone {
    tower::service_fn(|req: http::Request<SdkBody>| async move {
        let cert = fs::read_to_string("/Users/rcoh/.mitmproxy/mitmproxy-ca-cert.pem").expect("loading cert");
        let cert = Certificate::from_pem(cert.as_bytes()).expect("couldn't load cert");
        let client = reqwest::ClientBuilder::new().add_root_certificate(cert).build().expect("build client");
        let req = req.map(|body| Bytes::copy_from_slice(body.bytes().unwrap()));
        let req = reqwest::Request::try_from(req).map_err(|_| "not a request")?;
        let response = client.execute(req).await.unwrap();
        let mut builder = http::Response::builder().status(response.status());
        *(builder.headers_mut().unwrap()) = response.headers().clone();
        let data = response.bytes().await.unwrap();
        Result::<http::Response<hyper::Body>, BoxError>::Ok(
            builder
                .body(hyper::Body::from(data))
                .unwrap(),
        )
    })
}

#[tokio::test]
async fn test_list_tables() {
    let config = dynamodb::Config::builder()
        .region("us-east-1")
        .credentials_provider(Credentials::from_keys("asdf", "asdf", None))
        .build();
    let client = aws_hyper::Client::new(make_connector());
    let request = ListTables::builder().build(&config);

    start_test("list-tables");
    let response = client.call(request).await.expect("success response");
    assert_eq!(response.table_names.unwrap(), vec!["new_table".to_owned()]);
    let output = finish_test();
    assert_eq!(output.errors, Vec::<String>::new());
}

#[tokio::test]
async fn test_invalid_response() {
    let config = dynamodb::Config::builder()
        .region("us-east-1")
        .credentials_provider(Credentials::from_keys("asdf", "asdf", None))
        .build();
    let client = aws_hyper::Client::new(make_connector());
    let request = ListTables::builder().build(&config);

    start_test("invalid-response");
    let _ = client.call(request).await.expect_err("response does not contain valid JSON");
    let output = finish_test();
    assert_eq!(output.errors, Vec::<String>::new());
}
