/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This is the code used by CI to run the canary Lambda.
//
// If running this locally, you'll need to make a clone of awslabs/smithy-rs in
// the aws-sdk-rust project root.
//
// Also consider using the `AWS_PROFILE` and `AWS_REGION` environment variables
// when running this locally.
//
// CAUTION: This subcommand will `git reset --hard` in some cases. Don't ever run
// it against a smithy-rs repo that you're actively working in.

use anyhow::{bail, Context, Result};
use aws_sdk_cloudwatch as cloudwatch;
use aws_sdk_lambda as lambda;
use aws_sdk_s3 as s3;
use clap::Parser;
use cloudwatch::model::StandardUnit;
use s3::ByteStream;
use semver::Version;
use serde::Deserialize;
use smithy_rs_tool_common::git::{find_git_repository_root, Git, GitCLI};
use smithy_rs_tool_common::macros::here;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use std::{env, path::Path};
use tokio::process::Command;
use tracing::{error, info};

lazy_static::lazy_static! {
    // Occasionally, a breaking change introduced in smithy-rs will cause the canary to fail
    // for older versions of the SDK since the canary is in the smithy-rs repository and will
    // get fixed for that breaking change. When this happens, the older SDK versions can be
    // pinned to a commit hash in the smithy-rs repository to get old canary code that still
    // compiles against that version of the SDK.
    static ref PINNED_SMITHY_RS_VERSIONS: Vec<(Version, &'static str)> = {
        let mut pinned = vec![
            // Versions <= 0.6.0 no longer compile against the canary after this commit in smithy-rs
            // due to the breaking change in https://github.com/awslabs/smithy-rs/pull/1085
            (Version::parse("0.6.0").unwrap(), "d48c234796a16d518ca9e1dda5c7a1da4904318c"),
        ];
        pinned.sort();
        pinned
    };
}

#[derive(Debug, Parser)]
pub struct RunOpt {
    /// Version of the SDK to compile the canary against
    #[clap(long, required_unless_present = "sdk-path")]
    sdk_version: Option<String>,

    /// Path to the SDK to compile against
    #[clap(long, required_unless_present = "sdk-version")]
    sdk_path: Option<PathBuf>,

    /// Whether to target MUSL instead of GLIBC when compiling the Lambda
    #[clap(long)]
    musl: bool,

    /// File path to a CDK outputs JSON file. This can be used instead
    /// of all the --lambda... args.
    #[clap(long)]
    cdk_output: Option<PathBuf>,

    /// The name of the S3 bucket to upload the canary binary bundle to
    #[clap(long, required_unless_present = "cdk-output")]
    lambda_code_s3_bucket_name: Option<String>,

    /// The name of the S3 bucket for the canary Lambda to interact with
    #[clap(long, required_unless_present = "cdk-output")]
    lambda_test_s3_bucket_name: Option<String>,

    /// The ARN of the role that the Lambda will execute as
    #[clap(long, required_unless_present = "cdk-output")]
    lambda_execution_role_arn: Option<String>,
}

#[derive(Debug)]
struct Options {
    sdk_version: Option<String>,
    sdk_path: Option<PathBuf>,
    musl: bool,
    lambda_code_s3_bucket_name: String,
    lambda_test_s3_bucket_name: String,
    lambda_execution_role_arn: String,
}

impl Options {
    fn load_from(run_opt: RunOpt) -> Result<Options> {
        if let Some(cdk_output) = &run_opt.cdk_output {
            #[derive(Deserialize)]
            struct Inner {
                #[serde(rename = "canarycodebucketname")]
                lambda_code_s3_bucket_name: String,
                #[serde(rename = "canarytestbucketname")]
                lambda_test_s3_bucket_name: String,
                #[serde(rename = "lambdaexecutionrolearn")]
                lambda_execution_role_arn: String,
            }
            #[derive(Deserialize)]
            struct Outer {
                #[serde(rename = "aws-sdk-rust-canary-stack")]
                inner: Inner,
            }

            let value: Outer = serde_json::from_reader(
                std::fs::File::open(cdk_output).context("open cdk output")?,
            )
            .context("read cdk output")?;
            Ok(Options {
                sdk_version: run_opt.sdk_version,
                sdk_path: run_opt.sdk_path,
                musl: run_opt.musl,
                lambda_code_s3_bucket_name: value.inner.lambda_code_s3_bucket_name,
                lambda_test_s3_bucket_name: value.inner.lambda_test_s3_bucket_name,
                lambda_execution_role_arn: value.inner.lambda_execution_role_arn,
            })
        } else {
            Ok(Options {
                sdk_version: run_opt.sdk_version,
                sdk_path: run_opt.sdk_path,
                musl: run_opt.musl,
                lambda_code_s3_bucket_name: run_opt.lambda_code_s3_bucket_name.expect("required"),
                lambda_test_s3_bucket_name: run_opt.lambda_test_s3_bucket_name.expect("required"),
                lambda_execution_role_arn: run_opt.lambda_execution_role_arn.expect("required"),
            })
        }
    }
}

pub async fn run(opt: RunOpt) -> Result<()> {
    let options = Options::load_from(opt)?;
    let start_time = SystemTime::now();
    let config = aws_config::load_from_env().await;
    let result = run_canary(&options, &config).await;

    let mut metrics = vec![
        (
            "canary-success",
            if result.is_ok() { 1.0 } else { 0.0 },
            StandardUnit::Count,
        ),
        (
            "canary-failure",
            if result.is_ok() { 0.0 } else { 1.0 },
            StandardUnit::Count,
        ),
        (
            "canary-total-time",
            start_time.elapsed().expect("time in range").as_secs_f64(),
            StandardUnit::Seconds,
        ),
    ];
    if let Ok(invoke_time) = result {
        metrics.push((
            "canary-invoke-time",
            invoke_time.as_secs_f64(),
            StandardUnit::Seconds,
        ));
    }

    let cloudwatch_client = cloudwatch::Client::new(&config);
    let mut request_builder = cloudwatch_client
        .put_metric_data()
        .namespace("aws-sdk-rust-canary");
    for metric in metrics {
        request_builder = request_builder.metric_data(
            cloudwatch::model::MetricDatum::builder()
                .metric_name(metric.0)
                .value(metric.1)
                .timestamp(SystemTime::now().into())
                .unit(metric.2)
                .build(),
        );
    }

    info!("Emitting metrics...");
    request_builder
        .send()
        .await
        .context(here!("failed to emit metrics"))?;

    result.map(|_| ())
}

async fn run_canary(options: &Options, config: &aws_config::Config) -> Result<Duration> {
    let smithy_rs_root = find_git_repository_root("smithy-rs", ".").context(here!())?;
    let smithy_rs = GitCLI::new(&smithy_rs_root).context(here!())?;
    env::set_current_dir(smithy_rs_root.join("tools/ci-cdk/canary-lambda"))
        .context("failed to change working directory")?;

    if let Some(sdk_version) = &options.sdk_version {
        use_correct_revision(&smithy_rs, sdk_version)
            .context(here!("failed to select correct revision of smithy-rs"))?;
    }

    info!("Building the canary...");
    let bundle_path = build_bundle(options).await?;
    let bundle_file_name = bundle_path.file_name().unwrap().to_str().unwrap();
    let bundle_name = bundle_path.file_stem().unwrap().to_str().unwrap();

    let s3_client = s3::Client::new(config);
    let lambda_client = lambda::Client::new(config);

    info!("Uploading Lambda code bundle to S3...");
    upload_bundle(
        s3_client,
        &options.lambda_code_s3_bucket_name,
        bundle_file_name,
        &bundle_path,
    )
    .await
    .context(here!())?;

    info!(
        "Creating the canary Lambda function named {}...",
        bundle_name
    );
    create_lambda_fn(
        lambda_client.clone(),
        bundle_name,
        bundle_file_name,
        &options.lambda_execution_role_arn,
        &options.lambda_code_s3_bucket_name,
        &options.lambda_test_s3_bucket_name,
    )
    .await
    .context(here!())?;

    info!("Invoking the canary Lambda...");
    let invoke_start_time = SystemTime::now();
    let invoke_result = invoke_lambda(lambda_client.clone(), bundle_name).await;
    let invoke_time = invoke_start_time.elapsed().expect("time in range");

    info!("Deleting the canary Lambda...");
    delete_lambda_fn(lambda_client, bundle_name)
        .await
        .context(here!())?;

    invoke_result.map(|_| invoke_time)
}

fn use_correct_revision(smithy_rs: &dyn Git, sdk_version: &str) -> Result<()> {
    let sdk_version = Version::parse(sdk_version).expect("valid version");
    if let Some((version, commit_hash)) = PINNED_SMITHY_RS_VERSIONS
        .iter()
        .find(|(v, _)| v >= &sdk_version)
    {
        info!(
            "SDK version {} requires smithy-rs@{} to successfully compile the canary",
            version, commit_hash
        );
        // Reset to the revision rather than checkout since the very act of running the
        // canary-runner can make the working tree dirty by modifying the Cargo.lock file
        smithy_rs.hard_reset(commit_hash).context(here!())?;
    }
    Ok(())
}

/// Returns the path to the compiled bundle zip file
async fn build_bundle(options: &Options) -> Result<PathBuf> {
    let mut builder = Command::new("./build-bundle");
    if let Some(sdk_version) = &options.sdk_version {
        builder.arg("--sdk-version").arg(sdk_version);
    }
    if let Some(sdk_path) = &options.sdk_path {
        builder.arg("--sdk-path").arg(sdk_path);
    }
    if options.musl {
        builder.arg("--musl");
    }
    let output = builder
        .stderr(std::process::Stdio::inherit())
        .output()
        .await
        .context(here!())?;
    if !output.status.success() {
        error!(
            "{}",
            std::str::from_utf8(&output.stderr).expect("valid utf-8")
        );
        bail!("Failed to build the canary bundle");
    } else {
        Ok(PathBuf::from(
            String::from_utf8(output.stdout).context(here!())?.trim(),
        ))
    }
}

async fn upload_bundle(
    s3_client: s3::Client,
    s3_bucket: &str,
    file_name: &str,
    bundle_path: &Path,
) -> Result<()> {
    s3_client
        .put_object()
        .bucket(s3_bucket)
        .key(file_name)
        .body(
            ByteStream::from_path(bundle_path)
                .await
                .context(here!("failed to load bundle file"))?,
        )
        .send()
        .await
        .context(here!("failed to upload bundle to S3"))?;
    Ok(())
}

async fn create_lambda_fn(
    lambda_client: lambda::Client,
    bundle_name: &str,
    bundle_file_name: &str,
    execution_role: &str,
    code_s3_bucket: &str,
    test_s3_bucket: &str,
) -> Result<()> {
    use lambda::model::*;

    lambda_client
        .create_function()
        .function_name(bundle_name)
        .runtime(Runtime::Providedal2)
        .role(execution_role)
        .handler("aws-sdk-rust-lambda-canary")
        .code(
            FunctionCode::builder()
                .s3_bucket(code_s3_bucket)
                .s3_key(bundle_file_name)
                .build(),
        )
        .publish(true)
        .environment(
            Environment::builder()
                .variables("RUST_BACKTRACE", "1")
                .variables("CANARY_S3_BUCKET_NAME", test_s3_bucket)
                .variables(
                    "CANARY_EXPECTED_TRANSCRIBE_RESULT",
                    "Good day to you transcribe. This is Polly talking to you from the Rust ST K.",
                )
                .build(),
        )
        .timeout(60)
        .send()
        .await
        .context(here!("failed to create canary Lambda function"))?;

    let mut attempts = 0;
    let mut state = State::Pending;
    while !matches!(state, State::Active) && attempts < 20 {
        info!("Waiting 1 second for Lambda to become active...");
        tokio::time::sleep(Duration::from_secs(1)).await;
        let configuration = lambda_client
            .get_function_configuration()
            .function_name(bundle_name)
            .send()
            .await
            .context(here!("failed to get Lambda function status"))?;
        state = configuration.state.unwrap();
        attempts += 1;
    }
    if !matches!(state, State::Active) {
        bail!("Timed out waiting for canary Lambda to become active");
    }
    Ok(())
}

async fn invoke_lambda(lambda_client: lambda::Client, bundle_name: &str) -> Result<()> {
    use lambda::model::*;
    use lambda::Blob;

    let response = lambda_client
        .invoke()
        .function_name(bundle_name)
        .invocation_type(InvocationType::RequestResponse)
        .log_type(LogType::Tail)
        .payload(Blob::new(&b"{}"[..]))
        .send()
        .await
        .context(here!("failed to invoke the canary Lambda"))?;

    if let Some(log_result) = response.log_result {
        info!(
            "Last 4 KB of canary logs:\n----\n{}\n----\n",
            std::str::from_utf8(&base64::decode(&log_result)?)?
        );
    }
    if response.status_code != 200 {
        bail!(
            "Canary failed: {}",
            response
                .function_error
                .as_deref()
                .unwrap_or("<no error given>")
        );
    }
    Ok(())
}

async fn delete_lambda_fn(lambda_client: lambda::Client, bundle_name: &str) -> Result<()> {
    lambda_client
        .delete_function()
        .function_name(bundle_name)
        .send()
        .await
        .context(here!("failed to delete Lambda"))?;
    Ok(())
}
