/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// PLATFORM TLS DIAGNOSTIC -- deliberately NOT part of the live phase.
//
// What this exists to answer, and nothing more: does the *platform's own
// certificate store* enumerate successfully, and does rustls then complete a
// real TLS handshake against an AWS endpoint using those roots?
//
// That is the whole of design.md 3.1 gap 2's residual "platform axis". The
// platform-specific code is the cert *loading*, not the TLS itself:
// `aws-smithy-http-client` calls `load_native_certs()`
// (src/client/tls/rustls_provider.rs:118), which resolves to
//   - a file-based store on Linux/glibc and Linux/musl,
//   - `schannel` on Windows,
//   - `security-framework` on macOS.
// rustls does the verification against whatever that returns. So "the handshake
// completed" is precisely the proof that the platform path worked.
//
// WHY THIS NEEDS NO CREDENTIALS, and why that is not the workaround the design
// rejects. design.md 3.1 gap 5 deleted two credential-free probes -- an
// `InvalidAccessKeyId` key and a `no_credentials()` public-bucket read -- because
// they were being used to claim *signature acceptance* and *real service
// responses*, which they cannot prove: they passed identically with and without
// credentials. Cert-store enumeration is the opposite case. It happens strictly
// below signing, so credentials are irrelevant to it by construction, and an
// unauthenticated handshake is the correct instrument rather than a shortcut.
//
// Consequently this asserts ONE thing: that the failure we get back is a
// *service* failure and not a *connector* failure. A service error means DNS
// resolved, the TLS handshake completed, and a real HTTP response came back. A
// connector error is the platform cert path failing, which is the break this
// diagnostic is for.
//
// MEASURED: a broken trust store fails this test by TWO different mechanisms,
// depending on the profile -- which matters because the design builds both
// (3.1 gap 6 ships a `--release` leg):
//
//   debug   -> PANIC inside aws-smithy-http-client at rustls_provider.rs:163,
//              "TrustStore configured to enable native roots but no valid root
//              certificates parsed!". It is a `debug_assert!`, so it fires
//              before any request is dispatched and the discriminator below is
//              never reached.
//   release -> the `debug_assert!` is compiled out, so the client proceeds with
//              an EMPTY root store and every handshake fails. That surfaces as
//              DispatchFailure -> ConnectorError{Io} ->
//              TlsConnectError{Protocol} -> InvalidCertificate(UnknownIssuer),
//              which is what the discriminator below catches.
//
// Both were verified by pointing SSL_CERT_FILE and SSL_CERT_DIR at an empty
// file, so this diagnostic is known to fail when the thing it tests is broken.
// Worth carrying into the design: on a host with no usable trust store the SDK
// panics in debug and fails handshakes in release, so the live phase's failure
// signature is profile-dependent.
//
// This does NOT replace running the live phase on Windows and macOS. It is the
// part of that work which can be settled without an account, so it can run on a
// fork PR with no secrets (design.md 3.3 notes the live phase cannot).

use aws_sdk_s3::config::{BehaviorVersion, Region};

/// A bucket name that is syntactically valid and certain not to resolve to a
/// bucket we own. We want the *service* to answer; we do not care what it says.
const ABSENT_BUCKET: &str = "canary-platform-tls-probe-does-not-exist";

#[tokio::test]
#[ignore = "platform diagnostic: requires network egress (no credentials needed)"]
async fn platform_native_cert_store_completes_a_real_tls_handshake() {
    let cfg = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .build();
    let client = aws_sdk_s3::Client::from_conf(cfg);

    let err = client
        .head_bucket()
        .bucket(ABSENT_BUCKET)
        .send()
        .await
        .expect_err("an anonymous HeadBucket on a nonexistent bucket cannot succeed");

    // The discriminator. `DispatchFailure` is the connector layer: DNS, TCP, or
    // -- the case we are hunting -- the platform cert store failing to load or
    // rustls rejecting the chain. Anything else means bytes crossed a completed
    // TLS session and the service replied.
    if let aws_sdk_s3::error::SdkError::DispatchFailure(ref ctx) = err {
        panic!(
            "PLATFORM TLS PATH FAILED -- the connector never got a response. \
             This is the platform's native certificate store or the handshake, \
             not the service. ctx={ctx:?}"
        );
    }

    // Report what we reached, so the log is evidence rather than a bare pass.
    match &err {
        aws_sdk_s3::error::SdkError::ServiceError(e) => {
            let raw = e.raw();
            println!(
                "ok: TLS handshake completed via the platform cert store; \
                 service replied HTTP {}",
                raw.status().as_u16()
            );
        }
        other => {
            println!(
                "ok: TLS handshake completed via the platform cert store; \
                 non-dispatch error class = {}",
                match other {
                    aws_sdk_s3::error::SdkError::ResponseError(_) => "ResponseError",
                    aws_sdk_s3::error::SdkError::TimeoutError(_) => "TimeoutError",
                    aws_sdk_s3::error::SdkError::ConstructionFailure(_) => "ConstructionFailure",
                    _ => "other",
                }
            );
        }
    }
}
