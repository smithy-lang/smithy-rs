/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth::Credentials;
use aws_http::user_agent::AwsUserAgent;
use aws_hyper::test_connection::TestConnection;
use aws_hyper::Client;
use http::Uri;
use kms::operation::GenerateRandom;
use kms::{Config, Region};
use smithy_http::body::SdkBody;
use std::time::{Duration, UNIX_EPOCH};

// TODO: having the full HTTP requests right in the code is a bit gross, consider something
// like https://github.com/davidbarsky/sigv4/blob/master/aws-sigv4/src/lib.rs#L283-L315 to store
// the requests/responses externally

// NOTE: The credentials in this file are real, but they are expiring STS credentials with a minimum (15 minute) expiry
// and have long-since expired.

#[tokio::test]
async fn generate_random() {
    let creds = Credentials::from_keys("ASIAR6OFQKMAEQYT56O5", "HmWca4XpftIbSxvSrnrELgWzOk3IfjzDKtFBhDby", Some("IQoJb3JpZ2luX2VjENb//////////wEaCXVzLWVhc3QtMSJHMEUCIQDmkZeDxs+Yaad2Azm1ju0CpuIIuV1sh3SWJlwjCMZndAIgLgqKhABROA25aC1SSxkLU2WYZr9x1p+NP2lGuXC1sCwqmQII7///////////ARAAGgwxMzQwOTUwNjU4NTYiDLPObEvLEoyOGG0FNirtAY1yN/hBDvwspxamWo4RROGczfXEgqPOih6h2S9mKhjS5KzYriAxXlwqLGOcgmJmraPhV5PAWmkRvZx+mVPzgy8j/F19ys9HgIG6oMfRYBXG78/19aCdHfgJa6bwjWDRnsXxhOq3wdFDzoWX8sLjsMsbyKkbXmMKSg5OLhuAogBxAEE9ifErTqi8qNozuSZmGe45yySyJqUhIHkkwysocq5lc/BkJZWB8g0cLLkcBhoTjGWgp2MENqbVkK4ca8yuM2TWa9HquN15gYaALy+tp3OqPNc6a6DQdYbiIAIFxwtUkVz6MDSv0TxzDB0LyjDa7YiCBjqdAbDuwjL16uKCsF5U+dno+swkV9BpmVrUOmq7iPo4jGE0wL8vIRJgUkyZ4nb+mkJwM/KqmYBwFkcBqQmERKunL7Pjh3l6Jm4edoW+PIg3IJ2juEPUZ+LJXQr+e7DaiV7U54gBYH663ZcSvZ/LNo3VDRDaKndQbXgCgozG/T7Va3QXzRouWQuscO+WqXnO32OObvqJCtEhgoSgynK4I3o=".to_string()));
    let conn = TestConnection::new(vec![(http::Request::builder()
                                             .header("content-type", "application/x-amz-json-1.1")
                                             .header("x-amz-target", "TrentService.GenerateRandom")
                                             .header("content-length", "20")
                                             .header("host", "kms.us-east-1.amazonaws.com")
                                             .header("authorization", "AWS4-HMAC-SHA256 Credential=ASIAR6OFQKMAEQYT56O5/20210305/us-east-1/kms/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-target, Signature=f17e7972b4364b48118a7adfe0188f3a0be2d941f2f4b44d69ae50cb3d4aec9f")
                                             .header("x-amz-date", "20210305T134922Z")
                                             .header("x-amz-security-token", "IQoJb3JpZ2luX2VjENb//////////wEaCXVzLWVhc3QtMSJHMEUCIQDmkZeDxs+Yaad2Azm1ju0CpuIIuV1sh3SWJlwjCMZndAIgLgqKhABROA25aC1SSxkLU2WYZr9x1p+NP2lGuXC1sCwqmQII7///////////ARAAGgwxMzQwOTUwNjU4NTYiDLPObEvLEoyOGG0FNirtAY1yN/hBDvwspxamWo4RROGczfXEgqPOih6h2S9mKhjS5KzYriAxXlwqLGOcgmJmraPhV5PAWmkRvZx+mVPzgy8j/F19ys9HgIG6oMfRYBXG78/19aCdHfgJa6bwjWDRnsXxhOq3wdFDzoWX8sLjsMsbyKkbXmMKSg5OLhuAogBxAEE9ifErTqi8qNozuSZmGe45yySyJqUhIHkkwysocq5lc/BkJZWB8g0cLLkcBhoTjGWgp2MENqbVkK4ca8yuM2TWa9HquN15gYaALy+tp3OqPNc6a6DQdYbiIAIFxwtUkVz6MDSv0TxzDB0LyjDa7YiCBjqdAbDuwjL16uKCsF5U+dno+swkV9BpmVrUOmq7iPo4jGE0wL8vIRJgUkyZ4nb+mkJwM/KqmYBwFkcBqQmERKunL7Pjh3l6Jm4edoW+PIg3IJ2juEPUZ+LJXQr+e7DaiV7U54gBYH663ZcSvZ/LNo3VDRDaKndQbXgCgozG/T7Va3QXzRouWQuscO+WqXnO32OObvqJCtEhgoSgynK4I3o=")
                                             .header("user-agent", "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
                                             .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
                                             .uri(Uri::from_static("https://kms.us-east-1.amazonaws.com/"))
                                             .body(SdkBody::from(r#"{"NumberOfBytes":64}"#)).unwrap(), http::Response::builder()
                                             .status(http::StatusCode::from_u16(200).unwrap())
                                             .body(r#"{"Plaintext":"6CG0fbzzhg5G2VcFCPmJMJ8Njv3voYCgrGlp3+BZe7eDweCXgiyDH9BnkKvLmS7gQhnYDUlyES3fZVGwv5+CxA=="}"#).unwrap())]);
    let client = Client::new(conn.clone());
    let conf = Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(creds)
        .build();
    let mut op = GenerateRandom::builder().number_of_bytes(64).build(&conf);
    op.config_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1614952162));
    op.config_mut().insert(AwsUserAgent::for_tests());
    let resp = client.call(op).await.expect("request should succeed");
    // primitive checksum
    assert_eq!(
        resp.plaintext
            .expect("blob should exist")
            .as_ref()
            .iter()
            .map(|i| *i as u32)
            .sum::<u32>(),
        8562
    );
    assert_eq!(conn.requests().len(), 1);
    for validate_request in conn.requests().iter() {
        validate_request.assert_matches(vec![]);
    }
}

#[tokio::test]
async fn generate_random_malformed_response() {
    let creds = Credentials::from_keys("ASIAR6OFQKMAEQYT56O5", "HmWca4XpftIbSxvSrnrELgWzOk3IfjzDKtFBhDby", Some("IQoJb3JpZ2luX2VjENb//////////wEaCXVzLWVhc3QtMSJHMEUCIQDmkZeDxs+Yaad2Azm1ju0CpuIIuV1sh3SWJlwjCMZndAIgLgqKhABROA25aC1SSxkLU2WYZr9x1p+NP2lGuXC1sCwqmQII7///////////ARAAGgwxMzQwOTUwNjU4NTYiDLPObEvLEoyOGG0FNirtAY1yN/hBDvwspxamWo4RROGczfXEgqPOih6h2S9mKhjS5KzYriAxXlwqLGOcgmJmraPhV5PAWmkRvZx+mVPzgy8j/F19ys9HgIG6oMfRYBXG78/19aCdHfgJa6bwjWDRnsXxhOq3wdFDzoWX8sLjsMsbyKkbXmMKSg5OLhuAogBxAEE9ifErTqi8qNozuSZmGe45yySyJqUhIHkkwysocq5lc/BkJZWB8g0cLLkcBhoTjGWgp2MENqbVkK4ca8yuM2TWa9HquN15gYaALy+tp3OqPNc6a6DQdYbiIAIFxwtUkVz6MDSv0TxzDB0LyjDa7YiCBjqdAbDuwjL16uKCsF5U+dno+swkV9BpmVrUOmq7iPo4jGE0wL8vIRJgUkyZ4nb+mkJwM/KqmYBwFkcBqQmERKunL7Pjh3l6Jm4edoW+PIg3IJ2juEPUZ+LJXQr+e7DaiV7U54gBYH663ZcSvZ/LNo3VDRDaKndQbXgCgozG/T7Va3QXzRouWQuscO+WqXnO32OObvqJCtEhgoSgynK4I3o=".to_string()));
    let conn = TestConnection::new(vec![(http::Request::builder()
                                             .body(SdkBody::from(r#"{"NumberOfBytes":64}"#)).unwrap(), http::Response::builder()
                                             .status(http::StatusCode::from_u16(200).unwrap())
                                             // last `}` replaced with a space
                                             .body(r#"{"Plaintext":"6CG0fbzzhg5G2VcFCPmJMJ8Njv3voYCgrGlp3+BZe7eDweCXgiyDH9BnkKvLmS7gQhnYDUlyES3fZVGwv5+CxA==" "#).unwrap())]);
    let client = Client::new(conn.clone());
    let conf = Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(creds)
        .build();
    let op = GenerateRandom::builder().number_of_bytes(64).build(&conf);
    client.call(op).await.expect_err("response was malformed");
}

#[tokio::test]
async fn generate_random_modeled_error() {
    let creds = Credentials::from_keys("ASIAR6OFQKMAMJXWLKNQ", "EQbTFlITTVAl8fUrt6NTi4qzbRxZ/hJ+IfhUpOcl", Some("IQoJb3JpZ2luX2VjENf//////////wEaCXVzLWVhc3QtMSJHMEUCIBvpF2yl2cS53T78jOuUYusCbyF5f0yeycLM/jNIepmAAiEAgcP3U5+tllSU97srwAkDo/ZIR/VRiO7ge5fHesOxCFYqmQII8P//////////ARAAGgwxMzQwOTUwNjU4NTYiDIlrWao5Iuo8u0Vw8CrtAQAvKFS0MMzbXejI15szeish9YAuEfM6KvYMmv+mkqhcSqdu6/4lM+WK+uOWB7/6dI6sWC5aGzF2nCOjIxMkeixxbUkyrGXnPxtdsZQDcU9+Mt+36rf93nMSn99UBM8zjw2hNbYQoxWpA4HtOIIFYo1NDW45CuGoZHvXf2x4YSd/REyd4+0Zu/UH+Q1F0abFFdh+p/tMfIW2lXoquUQhOMNTiVt5r34vAJXl0pbYGzo1NzRcrj39u7f2GI0AwuBirRvwJS7BqMM81YhW2HAFZeTQchF/El22NjRfLUmXszEd9mlpMcSzf8JyUR3kczDziImCBjqdAXQRDFtwOlZA8xpgDb8d6FFBswFCeCL1ceTG0QaQOeMxxUn4XMccEUG/73V7PlaPtvSFVuKA1ISCnVwaYJdGQOZzgUsBnb+FO5aoWcQxgkyEcGsJb+06fZGw/W8PwrR58LXPZKD+hjsdKU2Pg/qK9DAlmm/UK0F1Er/R9UgMgriWQqk9qeefEn27irvq23SosSu/R+Zh61WePtsHzbM=".to_string()));
    let conf = Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(creds)
        .build();
    let conn = TestConnection::new(vec![(http::Request::builder()
                                             .header("content-type", "application/x-amz-json-1.1")
                                             .header("x-amz-target", "TrentService.GenerateRandom")
                                             .header("content-length", "56")
                                             .header("host", "kms.us-east-1.amazonaws.com")
                                             .header("authorization", "AWS4-HMAC-SHA256 Credential=ASIAR6OFQKMAMJXWLKNQ/20210305/us-east-1/kms/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-target, Signature=4fef0ab83318dcaaad40f46993f8446bcab89ce12c9cae429783496b8cee29c1")
                                             .header("x-amz-date", "20210305T144724Z")
                                             .header("x-amz-security-token", "IQoJb3JpZ2luX2VjENf//////////wEaCXVzLWVhc3QtMSJHMEUCIBvpF2yl2cS53T78jOuUYusCbyF5f0yeycLM/jNIepmAAiEAgcP3U5+tllSU97srwAkDo/ZIR/VRiO7ge5fHesOxCFYqmQII8P//////////ARAAGgwxMzQwOTUwNjU4NTYiDIlrWao5Iuo8u0Vw8CrtAQAvKFS0MMzbXejI15szeish9YAuEfM6KvYMmv+mkqhcSqdu6/4lM+WK+uOWB7/6dI6sWC5aGzF2nCOjIxMkeixxbUkyrGXnPxtdsZQDcU9+Mt+36rf93nMSn99UBM8zjw2hNbYQoxWpA4HtOIIFYo1NDW45CuGoZHvXf2x4YSd/REyd4+0Zu/UH+Q1F0abFFdh+p/tMfIW2lXoquUQhOMNTiVt5r34vAJXl0pbYGzo1NzRcrj39u7f2GI0AwuBirRvwJS7BqMM81YhW2HAFZeTQchF/El22NjRfLUmXszEd9mlpMcSzf8JyUR3kczDziImCBjqdAXQRDFtwOlZA8xpgDb8d6FFBswFCeCL1ceTG0QaQOeMxxUn4XMccEUG/73V7PlaPtvSFVuKA1ISCnVwaYJdGQOZzgUsBnb+FO5aoWcQxgkyEcGsJb+06fZGw/W8PwrR58LXPZKD+hjsdKU2Pg/qK9DAlmm/UK0F1Er/R9UgMgriWQqk9qeefEn27irvq23SosSu/R+Zh61WePtsHzbM=")
                                             .header("user-agent", "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
                                             .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
                                             .uri(Uri::from_static("https://kms.us-east-1.amazonaws.com/"))
                                             .body(SdkBody::from(r#"{"NumberOfBytes":64,"CustomKeyStoreId":"does not exist"}"#)).unwrap(), http::Response::builder()
                                             .status(http::StatusCode::from_u16(400).unwrap())
        .header("x-amzn-requestid", "bfe81a0a-9a08-4e71-9910-cdb5ab6ea3b6")
        .header("cache-control", "no-cache, no-store, must-revalidate, private")
        .header("expires", "0")
        .header("pragma", "no-cache")
        .header("date", "Fri, 05 Mar 2021 15:01:40 GMT")
        .header("content-type", "application/x-amz-json-1.1")
        .header("content-length", "44")

        .body(r#"{"__type":"CustomKeyStoreNotFoundException"}"#).unwrap()), ]);

    let mut op = GenerateRandom::builder()
        .number_of_bytes(64)
        .custom_key_store_id("does not exist")
        .build(&conf);

    op.config_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1614955644));
    op.config_mut().insert(AwsUserAgent::for_tests());
    let client = Client::new(conn.clone());
    let err = client.call(op).await.expect_err("key store doesn't exist");
    let inner = match err {
        aws_hyper::SdkError::ServiceError {
            err: kms::error::GenerateRandomError::CustomKeyStoreNotFoundError(e),
            ..
        } => e,
        other => panic!("Incorrect error received: {:}", other),
    };
    assert_eq!(inner.message, None);
    assert_eq!(conn.requests().len(), 1);
    for validate_request in conn.requests().iter() {
        validate_request.assert_matches(vec![]);
    }
}
