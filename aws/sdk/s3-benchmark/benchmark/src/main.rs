/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::latencies::Latencies;
use crate::multipart_get::get_object_multipart;
use crate::multipart_put::put_object_multipart;
use aws_sdk_s3 as s3;
use clap::Parser as _;
use s3::error::DisplayErrorContext;
use s3::primitives::ByteStream;
use std::error::Error as StdError;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::time;

mod latencies;
mod multipart_get;
mod multipart_put;

pub type BoxError = Box<dyn StdError + Send + Sync>;

pub const BENCH_KEY: &str = "s3_bench_file";

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

#[derive(Debug, clap::Parser)]
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
    let client = s3::Client::new(&config);

    let result = match args.bench {
        Bench::PutObject => benchmark_put_object(&client, &args).await,
        Bench::GetObject => benchmark_get_object(&client, &args).await,
        Bench::PutObjectMultipart => benchmark_put_object_multipart(&client, &args).await,
        Bench::GetObjectMultipart => benchmark_get_object_multipart(&client, &args).await,
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
    ($client:ident, $args:ident, setup => $setup:expr, operation => $operation:expr) => {{
        let test_file_path = generate_test_file($args)?;
        $setup($client, $args, &test_file_path).await?;

        let mut latencies = Latencies::new($args.size_bytes);
        for i in 0..$args.iterations {
            let start = time::Instant::now();
            $operation($client, $args, &test_file_path).await?;
            let latency = start.elapsed();
            latencies.push(latency);
            println!(
                "finished iteration {i} in {} seconds",
                latency.as_secs_f64()
            );
        }

        Ok(latencies)
    }};
}

async fn benchmark_put_object(client: &s3::Client, args: &Args) -> Result<Latencies, BoxError> {
    benchmark!(client, args, setup => no_setup, operation => put_object)
}

async fn benchmark_get_object(client: &s3::Client, args: &Args) -> Result<Latencies, BoxError> {
    async fn operation(client: &s3::Client, args: &Args, path: &Path) -> Result<(), BoxError> {
        let output = client
            .get_object()
            .bucket(&args.bucket)
            .key(BENCH_KEY)
            .send()
            .await?;
        let mut body = output.body.into_async_read();
        let mut file = tokio::fs::File::create(path).await?;
        tokio::io::copy(&mut body, &mut file).await?;
        Ok(())
    }
    benchmark!(client, args, setup => put_object_intelligent, operation => operation)
}

async fn benchmark_put_object_multipart(
    client: &s3::Client,
    args: &Args,
) -> Result<Latencies, BoxError> {
    benchmark!(client, args, setup => no_setup, operation => put_object_multipart)
}

async fn benchmark_get_object_multipart(
    client: &s3::Client,
    args: &Args,
) -> Result<Latencies, BoxError> {
    benchmark!(client, args, setup => put_object_intelligent, operation => get_object_multipart)
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

    process::Command::new("truncate")
        .arg("-s")
        .arg(format!("{}", args.size_bytes))
        .arg(&path)
        .output()?;

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
        put_object_multipart(client, args, path).await
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
