/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Measures the production event-stream body wrappers, without a network or decoder.

#[allow(dead_code)]
#[path = "../../../rust-runtime/inlineable/src/event_stream_message_timeout.rs"]
mod event_stream_message_timeout;
#[allow(dead_code)]
#[path = "../../../rust-runtime/inlineable/src/event_stream_poll_budget.rs"]
mod event_stream_poll_budget;

use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use http_body_1x::{Body, Frame};
use http_body_util::BodyExt;
use std::convert::Infallible;
use std::hint::black_box;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

struct ReadyBody {
    chunks: Vec<Bytes>,
    remaining: usize,
    next: usize,
}

impl Body for ReadyBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        self.remaining -= 1;
        let chunk = self.chunks[self.next].clone();
        self.next = (self.next + 1) % self.chunks.len();
        Poll::Ready(Some(Ok(Frame::data(chunk))))
    }
}

fn wrap(body: SdkBody, variant: &str) -> SdkBody {
    use event_stream_message_timeout::wrap_with_event_stream_timeouts;
    use event_stream_poll_budget::wrap_with_poll_budget;
    let timeout = Some(Duration::from_secs(3600));
    match variant {
        "disabled" => wrap_with_poll_budget(
            wrap_with_event_stream_timeouts(body, None, None, None, false),
            0,
        ),
        "budget_only" => wrap_with_poll_budget(body, 32),
        "before_fix" => wrap_with_event_stream_timeouts(
            wrap_with_poll_budget(body, 32),
            None,
            timeout,
            timeout,
            false,
        ),
        "after_fix" => wrap_with_poll_budget(
            wrap_with_event_stream_timeouts(body, None, timeout, timeout, false),
            32,
        ),
        other => panic!("unknown variant: {other}"),
    }
}

async fn trial(variant: &str, chunks: &[Bytes], frames: usize) -> Duration {
    let mut body = wrap(
        SdkBody::from_body_1_x(ReadyBody {
            chunks: chunks.to_vec(),
            remaining: frames,
            next: 0,
        }),
        variant,
    );
    let expected_bytes = frames / chunks.len() * chunks.iter().map(Bytes::len).sum::<usize>();
    // Exclude body construction and initial timer allocation from steady-state timing.
    let start = Instant::now();
    let mut observed_bytes = 0;
    while let Some(frame) = body.frame().await {
        observed_bytes += black_box(frame.unwrap().into_data().unwrap()).len();
    }
    let elapsed = start.elapsed();
    assert_eq!(observed_bytes, expected_bytes);
    elapsed
}

fn main() {
    let frames = std::env::args()
        .nth(1)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(1_048_576);
    let repeats = std::env::args()
        .nth(2)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(9);
    assert!(frames > 0 && frames % 16 == 0 && repeats > 0);
    // The tracker reads only total_length; CRC and payload validation are not measured.
    let mut frame = vec![0; 1024];
    frame[..4].copy_from_slice(&1024_u32.to_be_bytes());
    let frame = Bytes::from(frame);
    let workloads = [
        ("complete_1024", vec![frame.clone()]),
        (
            "fragmented_64",
            (0..16).map(|i| frame.slice(i * 64..(i + 1) * 64)).collect(),
        ),
    ];
    let variants = ["disabled", "budget_only", "before_fix", "after_fix"];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    println!(
        "workload,variant,repeat,http_frames,bytes,elapsed_ns,ns_per_http_frame,mib_per_second"
    );
    runtime.block_on(async {
        for (workload, chunks) in workloads {
            for variant in variants {
                trial(variant, &chunks, 16_384).await;
            }
            for repeat in 0..repeats {
                // Rotate and reverse the order to distribute drift across variants.
                for slot in 0..variants.len() {
                    let offset = if repeat % 2 == 0 { slot } else { 3 - slot };
                    let variant = variants[(repeat + offset) % variants.len()];
                    let elapsed = trial(variant, &chunks, frames).await;
                    let bytes =
                        frames / chunks.len() * chunks.iter().map(Bytes::len).sum::<usize>();
                    println!(
                        "{workload},{variant},{repeat},{frames},{bytes},{},{:.3},{:.3}",
                        elapsed.as_nanos(),
                        elapsed.as_nanos() as f64 / frames as f64,
                        bytes as f64 / 1_048_576.0 / elapsed.as_secs_f64(),
                    );
                }
            }
        }
    });
}
