// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
mod events;
mod metrics;
use bench_sdk::{input, output, BenchmarkService, BenchmarkServiceConfig};
use std::io::{BufRead, Write};

fn main() {
    let workers: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1".into())
        .parse()
        .unwrap();
    std::thread::spawn(|| {
        for line in std::io::stdin().lock().lines() {
            match line.unwrap().as_str() {
                "snapshot" => println!("{}", metrics::snapshot()),
                "reset" => {
                    metrics::reset_peak();
                    println!("{}", metrics::snapshot());
                }
                "quit" => std::process::exit(0),
                _ => panic!("unknown control command"),
            }
            std::io::stdout().flush().unwrap();
        }
    });
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let app = BenchmarkService::builder(BenchmarkServiceConfig::builder().build())
            .upload(|mut input: input::UploadInput| async move {
                let mut count = 0;
                while let Some(value) = input.events.recv().await.expect("valid upload event") {
                    events::verify(&value, &input.kind, input.size as usize, count);
                    count += 1;
                }
                assert_eq!(count, input.count as usize);
                Ok::<_, bench_sdk::error::UploadError>(output::UploadOutput::builder().count(count as i32).metadata(events::METADATA.into()).build().unwrap())
            })
            .download(|input: input::DownloadInput| async move {
                let stream = async_stream::stream! {
                    for i in 0..input.count as usize {
                        yield Ok(events::event(&input.kind, input.size as usize, i));
                    }
                    if input.fail {
                        yield Err(bench_sdk::error::EventsError::StreamFailure(bench_sdk::error::StreamFailure::builder().message(Some("expected stream failure".into())).build()));
                    }
                };
                Ok::<_, bench_sdk::error::DownloadError>(output::DownloadOutput::builder().metadata(events::METADATA.into()).events(stream.into()).build().unwrap())
            })
            .duplex(|mut input: input::DuplexInput| async move {
                let stream = async_stream::stream! {
                    let mut count = 0;
                    while let Some(value) = input.events.recv().await.expect("valid duplex event") {
                        events::verify(&value, &input.kind, input.size as usize, count);
                        count += 1;
                        yield Ok(value);
                    }
                    assert_eq!(count, input.count as usize);
                };
                Ok::<_, bench_sdk::error::DuplexError>(output::DuplexOutput::builder().metadata(events::METADATA.into()).events(stream.into()).build().unwrap())
            })
            .build().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        println!("READY:{}", listener.local_addr().unwrap().port());
        std::io::stdout().flush().unwrap();
        bench_sdk::serve(listener, app.into_make_service())
            .configure_hyper(|builder| builder.http2_only())
            .await.unwrap();
    });
}
