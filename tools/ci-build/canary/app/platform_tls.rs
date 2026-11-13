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
// That is the whole of the platform axis that the other legs cannot reach. The
// platform-specific code is the cert *loading*, not the TLS itself:
// `aws-smithy-http-client` calls `load_native_certs()`
// (src/client/tls/rustls_provider.rs:118), which resolves to
//   - a file-based store on Linux/glibc and Linux/musl,
//   - `schannel` on Windows,
//   - `security-framework` on macOS.
// rustls does the verification against whatever that returns. So "the handshake
// completed" is precisely the proof that the platform path worked.
//
// WHY THIS NEEDS NO CREDENTIALS, and why that is not the shortcut it resembles.
// Two other credential-free probes were tried and discarded -- an
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
// WHAT IS ACTUALLY MEASURED, AND WHAT IS NOT. Be careful with this one: an
// earlier version of this comment claimed more than the evidence supports.
//
// Measured, against aws-smithy-http-client 1.5.2 and rustls-native-certs 0.8.5:
//
//   * The probe does complete a real handshake. Anonymously, with no
//     credentials at all, S3 answers HeadBucket on an absent bucket with HTTP
//     404 -- so bytes crossed a finished TLS session and the discriminator
//     below is reached with a ServiceError.
//   * It does fail when the connector cannot get a response. Verified by
//     pointing the client through a dead proxy (HTTPS_PROXY to a closed port):
//     DispatchFailure, and the panic below fires.
//
// NOT measured, and the gap worth knowing about: breaking the *platform trust
// store* specifically. Setting SSL_CERT_FILE=/dev/null and
// SSL_CERT_DIR=/nonexistent does NOT break it -- the probe still gets its 404,
// so rustls-native-certs is evidently not taking its roots from those variables
// here. So "fails when the thing it tests is broken" is established only for
// *unreachability*, which is the weaker and much more common cause. Whether a
// genuinely broken native cert store would be caught is UNPROVEN, and that
// matters: unreachability is a property of the runner, while a broken cert
// store would be a property of the release. Finding a way to induce the latter
// is what would make this diagnostic worth arming.
//
// This does NOT replace running a credentialed live phase on Windows. It is the
// part of that work which can be settled without an account, so unlike the live
// phase it can run on a fork PR with no secrets.

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

    // The discriminator, and the only thing that counts as a pass is positive
    // evidence that a response came back: `ServiceError` means the service
    // answered, and `ResponseError` means it answered with something
    // unparseable. Either way the handshake finished.
    //
    // Everything else is treated as a connector failure. `ConstructionFailure`
    // means the request was never even built, and `TimeoutError` means nothing
    // came back. Neither is evidence of a handshake, and announcing one is how a
    // diagnostic ends up asserting the opposite of what it prints.
    match &err {
        aws_sdk_s3::error::SdkError::ServiceError(e) => {
            println!(
                "ok: TLS handshake completed via the platform cert store; \
                 service replied HTTP {}",
                e.raw().status().as_u16()
            );
        }
        aws_sdk_s3::error::SdkError::ResponseError(_) => {
            println!(
                "ok: TLS handshake completed via the platform cert store; \
                 a response arrived but did not parse"
            );
        }
        // CANARY_CONNECTOR_FAIL is read by the verdict classifier, which turns
        // this into INFRA_FAIL rather than RUNTIME_FAIL. The reason is in the
        // comment at the top of this file: what this reliably detects is an
        // unreachable network, which is a property of the runner and not of the
        // published crates.
        other => {
            panic!(
                "CANARY_CONNECTOR_FAIL: no response reached us, so no TLS \
                 handshake is demonstrated. This is the connector -- the \
                 network, or the platform's native certificate store -- and \
                 not the service. class={}, err={other:?}",
                match other {
                    aws_sdk_s3::error::SdkError::DispatchFailure(_) => "DispatchFailure",
                    aws_sdk_s3::error::SdkError::TimeoutError(_) => "TimeoutError",
                    aws_sdk_s3::error::SdkError::ConstructionFailure(_) => "ConstructionFailure",
                    _ => "other",
                }
            );
        }
    }
}
