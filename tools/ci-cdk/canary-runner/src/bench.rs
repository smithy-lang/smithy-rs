/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use clap::Parser;
use s3::primitives::ByteStream;
use smithy_rs_tool_common::macros::here;
use tracing::info;

use crate::arch::Arch;
use crate::build_bundle::BuildBundleArgs;

use aws_sdk_lambda as lambda;
use aws_sdk_lambda::client::Waiters;
use aws_sdk_s3 as s3;

#[derive(Debug, Parser, Eq, PartialEq)]
pub struct BenchArgs {
    /// SDK path to compile against
    #[clap(long)]
    sdk_path: PathBuf,

    /// File path to a CDK outputs JSON file
    #[clap(long)]
    cdk_output: Option<PathBuf>,

    /// S3 bucket to upload the canary bundle to
    #[clap(long, required_unless_present = "cdk-output")]
    lambda_code_s3_bucket_name: Option<String>,

    /// S3 bucket for the canary Lambda to interact with
    #[clap(long, required_unless_present = "cdk-output")]
    lambda_test_s3_bucket_name: Option<String>,

    /// ARN of the role that the Lambda will execute as
    #[clap(long, required_unless_present = "cdk-output")]
    lambda_execution_role_arn: Option<String>,

    /// Lambda architecture
    #[clap(long, default_value = "x86_64")]
    architecture: Arch,

    /// Whether to target MUSL instead of GLIBC
    #[clap(long)]
    musl: bool,

    /// Disable jitter entropy in AWS-LC
    #[clap(long)]
    disable_jitter_entropy: bool,

    /// Number of cold-start iterations
    #[clap(long, default_value = "100")]
    iterations: u32,

    /// Memory allocated for the Lambda function in MB
    #[clap(long, default_value = "512")]
    memory_size: i32,
}

struct BenchOptions {
    sdk_path: PathBuf,
    lambda_code_s3_bucket_name: String,
    lambda_test_s3_bucket_name: String,
    lambda_execution_role_arn: String,
    architecture: Arch,
    musl: bool,
    disable_jitter_entropy: bool,
    iterations: u32,
    memory_size: i32,
}

impl BenchOptions {
    fn load_from(args: BenchArgs) -> Result<Self> {
        let (lambda_code_s3_bucket_name, lambda_test_s3_bucket_name, lambda_execution_role_arn) =
            if let Some(cdk_output) = &args.cdk_output {
                #[derive(serde::Deserialize)]
                struct Inner {
                    #[serde(rename = "canarycodebucketname")]
                    lambda_code_s3_bucket_name: String,
                    #[serde(rename = "canarytestbucketname")]
                    lambda_test_s3_bucket_name: String,
                    #[serde(rename = "lambdaexecutionrolearn")]
                    lambda_execution_role_arn: String,
                }
                #[derive(serde::Deserialize)]
                enum Outer {
                    #[serde(rename = "aws-sdk-rust-canary-stack")]
                    AwsSdkRust {
                        #[serde(flatten)]
                        inner: Inner,
                    },
                    #[serde(rename = "smithy-rs-canary-stack")]
                    SmithyRs {
                        #[serde(flatten)]
                        inner: Inner,
                    },
                }
                impl Outer {
                    fn into_inner(self) -> Inner {
                        match self {
                            Outer::AwsSdkRust { inner } | Outer::SmithyRs { inner } => inner,
                        }
                    }
                }
                let value: Outer = serde_json::from_reader(
                    std::fs::File::open(cdk_output).context("open cdk output")?,
                )
                .context("read cdk output")?;
                let inner = value.into_inner();
                (
                    inner.lambda_code_s3_bucket_name,
                    inner.lambda_test_s3_bucket_name,
                    inner.lambda_execution_role_arn,
                )
            } else {
                (
                    args.lambda_code_s3_bucket_name.expect("required"),
                    args.lambda_test_s3_bucket_name.expect("required"),
                    args.lambda_execution_role_arn.expect("required"),
                )
            };

        Ok(Self {
            sdk_path: args.sdk_path,
            lambda_code_s3_bucket_name,
            lambda_test_s3_bucket_name,
            lambda_execution_role_arn,
            architecture: args.architecture,
            musl: args.musl,
            disable_jitter_entropy: args.disable_jitter_entropy,
            iterations: args.iterations,
            memory_size: args.memory_size,
        })
    }
}

pub async fn bench(args: BenchArgs) -> Result<()> {
    let opts = BenchOptions::load_from(args)?;

    let smithy_rs_root =
        smithy_rs_tool_common::git::find_git_repository_root("smithy-rs", ".").context(here!())?;
    std::env::set_current_dir(smithy_rs_root.join("tools/ci-cdk/canary-lambda"))
        .context("failed to change working directory")?;

    let config = aws_config::load_from_env().await;
    let s3_client = s3::Client::new(&config);
    let lambda_client = lambda::Client::new(&config);

    info!("Building canary bundle...");
    let bundle_path = build_bundle(&opts).await?;
    let bundle_file_name = bundle_path.file_name().unwrap().to_str().unwrap();
    let bundle_name = bundle_path.file_stem().unwrap().to_str().unwrap();

    info!("Uploading bundle to S3...");
    upload_bundle(
        &s3_client,
        &opts.lambda_code_s3_bucket_name,
        bundle_file_name,
        &bundle_path,
    )
    .await?;

    info!("Creating Lambda function {}...", bundle_name);
    create_lambda_fn(&lambda_client, bundle_name, bundle_file_name, &opts).await?;

    info!("Running {} cold start iterations...", opts.iterations);
    let result = run_benchmark(&lambda_client, bundle_name, &opts).await;

    // Cleanup
    info!("Deleting Lambda function...");
    lambda_client
        .delete_function()
        .function_name(bundle_name)
        .send()
        .await
        .context(here!("failed to delete Lambda"))?;

    result
}

async fn build_bundle(opts: &BenchOptions) -> Result<PathBuf> {
    let build_args = BuildBundleArgs {
        canary_path: None,
        rust_version: None,
        sdk_release_tag: None,
        sdk_path: Some(opts.sdk_path.clone()),
        musl: opts.musl,
        architecture: opts.architecture,
        manifest_only: false,
        disable_jitter_entropy: opts.disable_jitter_entropy,
        feature_override: Some("lambda-benchmark".to_string()),
    };
    Ok(crate::build_bundle::build_bundle(build_args)
        .await?
        .expect("manifest_only is false"))
}

async fn upload_bundle(s3_client: &s3::Client, bucket: &str, key: &str, path: &Path) -> Result<()> {
    s3_client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(ByteStream::from_path(path).await.context(here!())?)
        .send()
        .await
        .context(here!("failed to upload bundle to S3"))?;
    Ok(())
}

async fn create_lambda_fn(
    lambda_client: &lambda::Client,
    bundle_name: &str,
    bundle_file_name: &str,
    opts: &BenchOptions,
) -> Result<()> {
    use lambda::types::*;

    lambda_client
        .create_function()
        .function_name(bundle_name)
        .runtime(Runtime::Providedal2)
        .role(&opts.lambda_execution_role_arn)
        .handler("aws-sdk-rust-lambda-canary")
        .code(
            FunctionCode::builder()
                .s3_bucket(&opts.lambda_code_s3_bucket_name)
                .s3_key(bundle_file_name)
                .build(),
        )
        .publish(true)
        .environment(
            Environment::builder()
                .variables("RUST_BACKTRACE", "1")
                .variables("RUST_LOG", "info")
                .variables("CANARY_S3_BUCKET_NAME", &opts.lambda_test_s3_bucket_name)
                .build(),
        )
        .timeout(180)
        .memory_size(opts.memory_size)
        .architectures(opts.architecture.into())
        .send()
        .await
        .context(here!("failed to create Lambda function"))?;

    lambda_client
        .wait_until_function_active_v2()
        .function_name(bundle_name)
        .wait(Duration::from_secs(60))
        .await
        .context(here!("timed out waiting for Lambda to become active"))?;

    Ok(())
}

async fn run_benchmark(
    lambda_client: &lambda::Client,
    bundle_name: &str,
    opts: &BenchOptions,
) -> Result<()> {
    let mut init_durations = Vec::new();
    let mut durations = Vec::new();
    let mut failed_cold_starts = 0;

    for i in 0..opts.iterations {
        info!("Cold start invocation {}/{}", i + 1, opts.iterations);

        // Update env to force a new execution environment
        let random_value = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time in range")
            .as_nanos()
            .to_string();

        lambda_client
            .update_function_configuration()
            .function_name(bundle_name)
            .environment(
                lambda::types::Environment::builder()
                    .variables("RUST_BACKTRACE", "1")
                    .variables("RUST_LOG", "info")
                    .variables("CANARY_S3_BUCKET_NAME", &opts.lambda_test_s3_bucket_name)
                    .variables("CANARY_RANDOM", random_value)
                    .build(),
            )
            .send()
            .await
            .context(here!("failed to update Lambda configuration"))?;

        lambda_client
            .wait_until_function_updated()
            .function_name(bundle_name)
            .wait(Duration::from_secs(60))
            .await
            .context(here!("failed to wait for Lambda update"))?;

        match invoke_until_cold_start(lambda_client, bundle_name).await {
            Ok(timings) => {
                init_durations.push(timings.init_duration);
                durations.push(timings.duration);
            }
            Err(e) => {
                if e.to_string().contains("Failed to get cold start") {
                    failed_cold_starts += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }

    if !init_durations.is_empty() {
        info!("");
        print_stats("Init Duration", &init_durations);
        print_stats("Duration", &durations);
    }
    if failed_cold_starts > 0 {
        info!(
            "Failed to achieve cold start: {} invocations",
            failed_cold_starts
        );
    }
    Ok(())
}

struct LambdaTimings {
    init_duration: f64,
    duration: f64,
}

async fn invoke_until_cold_start(
    lambda_client: &lambda::Client,
    bundle_name: &str,
) -> Result<LambdaTimings> {
    const MAX_ATTEMPTS: u32 = 10;

    for attempt in 1..=MAX_ATTEMPTS {
        let timings = invoke_lambda(lambda_client, bundle_name).await?;
        if let Some(t) = timings {
            if attempt > 1 {
                info!("Cold start confirmed on attempt {}", attempt);
            }
            return Ok(t);
        }
        info!(
            "Warm start detected (attempt {}/{}), retrying...",
            attempt, MAX_ATTEMPTS
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    anyhow::bail!("Failed to get cold start after {} attempts", MAX_ATTEMPTS)
}

async fn invoke_lambda(
    lambda_client: &lambda::Client,
    bundle_name: &str,
) -> Result<Option<LambdaTimings>> {
    use lambda::primitives::Blob;
    use lambda::types::*;

    let response = lambda_client
        .invoke()
        .function_name(bundle_name)
        .invocation_type(InvocationType::RequestResponse)
        .log_type(LogType::Tail)
        .payload(Blob::new(&b"{}"[..]))
        .send()
        .await
        .context(here!("failed to invoke Lambda"))?;

    let mut timings = None;
    if let Some(log_result) = response.log_result() {
        let decoded = base64::decode(log_result)?;
        let logs = std::str::from_utf8(&decoded)?;
        info!("Last 4 KB of logs:\n----\n{}\n----\n", logs);

        if let (Some(init_duration), Some(duration)) = (
            parse_report_field(logs, "Init Duration:"),
            parse_report_duration(logs),
        ) {
            timings = Some(LambdaTimings {
                init_duration,
                duration,
            });
        }
    }

    if response.status_code() != 200 || response.function_error().is_some() {
        anyhow::bail!(
            "Lambda failed: {}",
            response.function_error.as_deref().unwrap_or("<no error>")
        );
    }

    Ok(timings)
}

fn parse_report_field(logs: &str, field: &str) -> Option<f64> {
    for line in logs.lines() {
        if let Some(pos) = line.find(field) {
            let after = &line[pos + field.len()..];
            if let Some(num_str) = after.split_whitespace().next() {
                return num_str.parse::<f64>().ok();
            }
        }
    }
    None
}

/// Parse "Duration: X.XX ms" from the REPORT line, avoiding "Init Duration:" and "Billed Duration:"
fn parse_report_duration(logs: &str) -> Option<f64> {
    for line in logs.lines() {
        if !line.starts_with("REPORT") {
            continue;
        }
        for field in line.split('\t') {
            let trimmed = field.trim();
            if trimmed.starts_with("Duration:") {
                return trimmed
                    .strip_prefix("Duration:")?
                    .split_whitespace()
                    .next()?
                    .parse::<f64>()
                    .ok();
            }
        }
    }
    None
}

fn print_stats(label: &str, durations: &[f64]) {
    let mut sorted = durations.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let p50 = sorted[sorted.len() * 50 / 100];
    let p90 = sorted[sorted.len() * 90 / 100];
    let p99 = sorted[sorted.len() * 99 / 100];

    let variance = sorted.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / sorted.len() as f64;
    let std_dev = variance.sqrt();

    info!("=== {label} (ms) ===");
    info!("Samples: {}", sorted.len());
    info!("Mean: {:.2}", mean);
    info!("P50: {:.2}", p50);
    info!("P90: {:.2}", p90);
    info!("P99: {:.2}", p99);
    info!("Std Dev: {:.2}", std_dev);
    info!("");
}
