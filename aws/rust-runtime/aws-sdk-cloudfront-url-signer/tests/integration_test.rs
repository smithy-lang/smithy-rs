/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_cloudfront_url_signer::{sign_cookies, sign_url, PrivateKey, SigningRequest};
use aws_smithy_types::DateTime;

const TEST_RSA_KEY: &[u8] = b"-----BEGIN RSA PRIVATE KEY-----
MIIBPAIBAAJBANW8WjQksUoX/7nwOfRDNt1XQpLCueHoXSt91MASMOSAqpbzZvXO
g2hW2gCFUIFUPCByMXPoeRe6iUZ5JtjepssCAwEAAQJBALR7ybwQY/lKTLKJrZab
D4BXCCt/7ZFbMxnftsC+W7UHef4S4qFW8oOOLeYfmyGZK1h44rXf2AIp4PndKUID
1zECIQD1suunYw5U22Pa0+2dFThp1VMXdVbPuf/5k3HT2/hSeQIhAN6yX0aT/N6G
gb1XlBKw6GQvhcM0fXmP+bVXV+RtzFJjAiAP+2Z2yeu5u1egeV6gdCvqPnUcNobC
FmA/NMcXt9xMSQIhALEMMJEFAInNeAIXSYKeoPNdkMPDzGnD3CueuCLEZCevAiEA
j+KnJ7pJkTvOzFwE8RfNLli9jf6/OhyYaLL4et7Ng5k=
-----END RSA PRIVATE KEY-----";

const TEST_ECDSA_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg4//aTM1/HqiVWagy
01cAx3EaegJ0Y5KLRoTtub8T8EWhRANCAARV/wa477wYpyWB5LCrCdS5M9bEAvD+
VORtjoydSpheKlsa+gE4PcFG88G2gE1Lilb8f6wEq/Lz+5kFa2S8gZmb
-----END PRIVATE KEY-----";

#[test]
fn test_sign_url_with_rsa_key() {
    let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
    let request = SigningRequest::builder()
        .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg")
        .key_pair_id("APKAEXAMPLE")
        .private_key(key)
        .expires_at(DateTime::from_secs(1767290400))
        .build()
        .unwrap();

    let signed_url = sign_url(request).unwrap();
    assert!(signed_url.as_str().contains("Signature="));
}

#[test]
fn test_sign_url_with_ecdsa_key() {
    let key = PrivateKey::from_pem(TEST_ECDSA_KEY).unwrap();
    let request = SigningRequest::builder()
        .resource_url("https://d111111abcdef8.cloudfront.net/image.jpg")
        .key_pair_id("APKAEXAMPLE")
        .private_key(key)
        .expires_at(DateTime::from_secs(1767290400))
        .build()
        .unwrap();

    let signed_url = sign_url(request).unwrap();
    assert!(signed_url.as_str().contains("Signature="));
}

#[test]
fn test_sign_cookies_with_rsa_key() {
    let key = PrivateKey::from_pem(TEST_RSA_KEY).unwrap();
    let request = SigningRequest::builder()
        .resource_url("https://d111111abcdef8.cloudfront.net/*")
        .key_pair_id("APKAEXAMPLE")
        .private_key(key)
        .expires_at(DateTime::from_secs(1767290400))
        .build()
        .unwrap();

    let cookies = sign_cookies(request).unwrap();
    assert!(cookies.get("CloudFront-Signature").is_some());
}

#[test]
fn test_sign_cookies_with_ecdsa_key() {
    let key = PrivateKey::from_pem(TEST_ECDSA_KEY).unwrap();
    let request = SigningRequest::builder()
        .resource_url("https://d111111abcdef8.cloudfront.net/*")
        .key_pair_id("APKAEXAMPLE")
        .private_key(key)
        .expires_at(DateTime::from_secs(1767290400))
        .build()
        .unwrap();

    let cookies = sign_cookies(request).unwrap();
    assert!(cookies.get("CloudFront-Signature").is_some());
}
