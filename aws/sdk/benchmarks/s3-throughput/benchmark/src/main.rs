/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::latencies::Latencies;
use crate::multipart_put::{put_object_multipart, PutObjectMultipart};
use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_s3 as s3;
use aws_sdk_s3::Client;
use clap::Parser as _;
use s3::error::DisplayErrorContext;
use s3::primitives::ByteStream;
use std::error::Error as StdError;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::process::{Command, Stdio};
use std::time;

mod get_test;
mod latencies;
mod multipart_get;
mod multipart_put;
mod put_test;
mod verify;

pub type BoxError = Box<dyn StdError + Send + Sync>;

pub const BENCH_KEY: &str = "s3_bench_file";

use crate::get_test::GetBenchmark;
use crate::put_test::PutBenchmark;
use tracing::Instrument;

#[derive(Copy, Clone, Debug, clap::ValueEnum)]
pub enum Fs {
    #[cfg(target_os = "linux")]
    // Use tmpfs
    Tmpfs,
    // Use the disk
    Disk,
}

#[derive(Copy, Clone, Debug, clap::ValueEnum)]
pub enum Bench {
    PutObject,
    GetObject,
    PutObjectMultipart,
    GetObjectMultipart,
}

#[derive(Debug, Clone, clap::Parser)]
#[command()]
pub struct Args {
    /// Which benchmark to run.
    #[arg(long)]
    bench: Bench,

    /// Local FS type to use.
    #[arg(long)]
    fs: Fs,

    /// Size of the object to benchmark with.
    #[arg(long)]
    size_bytes: u64,

    /// S3 bucket to test against.
    #[arg(long)]
    bucket: String,

    /// AWS region to use. Defaults to us-east-1.
    #[arg(long, default_value = "us-east-1")]
    region: String,

    /// AWS credentials profile to use.
    #[arg(long)]
    profile: Option<String>,

    /// Part size for multipart benchmarks. Defaults to 8 MiB.
    #[arg(long, default_value_t = 8_388_608)]
    part_size_bytes: u64,

    /// Number of benchmark iterations to perform.
    #[arg(long, default_value_t = 8)]
    iterations: usize,

    /// Number of concurrent uploads/downloads to perform.
    #[arg(long, default_value_t = 4)]
    concurrency: usize,

    #[arg(long, default_value_t = 1000)]
    part_upload_timeout_millis: u64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let config = {
        let mut loader =
            aws_config::from_env().region(s3::config::Region::new(args.region.clone()));
        if let Some(profile) = args.profile.as_ref() {
            loader = loader.profile_name(profile);
        }
        loader.load().await
    };

    let result = match args.bench {
        Bench::PutObject => benchmark_put_object(&config, &args).await,
        Bench::GetObject => benchmark_get_object(&config, &args).await,
        Bench::PutObjectMultipart => benchmark_put_object_multipart(&config, &args).await,
        Bench::GetObjectMultipart => benchmark_get_object_multipart(&config, &args).await,
    };
    match result {
        Ok(latencies) => {
            println!("benchmark succeeded");
            println!("=============== {:?} Result ================", args.bench);
            println!("{latencies}");
            println!("==========================================================");
        }
        Err(err) => {
            println!("benchmark failed: {}", DisplayErrorContext(err.as_ref()));
            process::exit(1);
        }
    }
}

macro_rules! benchmark {
    ($sdk_config:ident, $args:ident, setup => $setup:expr, operation => $operation:expr) => {{
        #[allow(unused)]
        use crate::get_test::GetBenchmark;
        #[allow(unused)]
        use crate::put_test::PutBenchmark;
        println!("setting up...");
        let test_file_path = generate_test_file($args)?;
        let setup_client = aws_sdk_s3::Client::new(&$sdk_config);
        $setup(&setup_client, $args, &test_file_path).await?;
        println!("setup complete");

        let mut latencies = Latencies::new($args.size_bytes);
        for i in 0..$args.iterations {
            let span = tracing::info_span!("run operation");
            let bench = $operation;
            let client = bench.prepare($sdk_config).await;
            let start = time::Instant::now();
            let result = bench
                .do_bench(client, $args, &test_file_path)
                .instrument(span)
                .await?;
            let latency = start.elapsed();
            if let Err(e) = bench.verify(&setup_client, $args, result).await {
                println!("benchmark did not finish correctly: {}", e);
            }
            latencies.push(latency);
            println!(
                "finished iteration {i} in {} seconds",
                latency.as_secs_f64()
            );
        }

        Ok(latencies)
    }};
}

async fn benchmark_put_object(conf: &SdkConfig, args: &Args) -> Result<Latencies, BoxError> {
    struct PutObject;
    #[async_trait]
    impl PutBenchmark for PutObject {
        type Setup = Client;

        async fn prepare(&self, conf: &SdkConfig) -> Self::Setup {
            Client::new(conf)
        }

        async fn do_put(
            &self,
            state: Self::Setup,
            target_key: &str,
            local_file: &Path,
            args: &Args,
        ) -> Result<(), BoxError> {
            state
                .put_object()
                .bucket(&args.bucket)
                .key(target_key)
                .body(ByteStream::from_path(local_file).await?)
                .send()
                .await?;
            Ok(())
        }
    }
    benchmark!(conf, args, setup => no_setup, operation => PutObject)
}

async fn benchmark_get_object(client: &SdkConfig, args: &Args) -> Result<Latencies, BoxError> {
    struct GetObject;
    #[async_trait]
    impl GetBenchmark for GetObject {
        type Setup = Client;

        async fn prepare(&self, conf: &SdkConfig) -> Self::Setup {
            Client::new(&conf)
        }

        async fn do_get(
            &self,
            state: Self::Setup,
            target_path: &Path,
            args: &Args,
        ) -> Result<PathBuf, BoxError> {
            let output = state
                .get_object()
                .bucket(&args.bucket)
                .key(BENCH_KEY)
                .send()
                .await?;
            let mut body = output.body.into_async_read();
            let mut file = tokio::fs::File::create(target_path).await?;
            tokio::io::copy(&mut body, &mut file).await?;
            Ok(target_path.to_path_buf())
        }
    }
    benchmark!(client, args, setup => put_object_intelligent, operation => GetObject)
}

async fn benchmark_put_object_multipart(
    conf: &SdkConfig,
    args: &Args,
) -> Result<Latencies, BoxError> {
    benchmark!(conf, args, setup => no_setup, operation => PutObjectMultipart)
}

async fn benchmark_get_object_multipart(
    config: &SdkConfig,
    args: &Args,
) -> Result<Latencies, BoxError> {
    benchmark!(config, args, setup => put_object_intelligent, operation => multipart_get::GetObjectMultipart::new())
}

fn generate_test_file(args: &Args) -> Result<PathBuf, BoxError> {
    let path = match args.fs {
        Fs::Disk => format!("/tmp/{BENCH_KEY}").into(),
        #[cfg(target_os = "linux")]
        Fs::Tmpfs => {
            if !PathBuf::from("/dev/shm").exists() {
                return Err("tmpfs not available on this machine".into());
            }
            format!("/dev/shm/{BENCH_KEY}").into()
        }
    };

    let mut yes_process = Command::new("yes")
        .arg("01234567890abcdefghijklmnopqrstuvwxyz")
        .stdout(Stdio::piped())
        .spawn()?;

    let mut head_process = Command::new("head")
        .arg("-c")
        .arg(format!("{}", args.size_bytes))
        .stdin(yes_process.stdout.take().unwrap())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut file = std::fs::File::create(&path)?;
    head_process.stdout.as_mut().unwrap();
    std::io::copy(&mut head_process.stdout.take().unwrap(), &mut file)?;

    let exit_status = head_process.wait()?;

    if !exit_status.success() {
        Err("failed to generate temp file")?
    }

    Ok(path)
}

async fn no_setup(_client: &s3::Client, _args: &Args, _path: &Path) -> Result<(), BoxError> {
    Ok(())
}

async fn put_object_intelligent(
    client: &s3::Client,
    args: &Args,
    path: &Path,
) -> Result<(), BoxError> {
    if args.size_bytes > args.part_size_bytes {
        put_object_multipart(&[client.clone()], args, BENCH_KEY, path).await
    } else {
        put_object(client, args, path).await
    }
}

async fn put_object(client: &s3::Client, args: &Args, path: &Path) -> Result<(), BoxError> {
    client
        .put_object()
        .bucket(&args.bucket)
        .key(BENCH_KEY)
        .body(ByteStream::from_path(&path).await?)
        .send()
        .await?;
    Ok(())
}
