use aws_smithy_async::future::fn_stream::FnStream;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_http::minimum_throughput::MinimumThroughputBody;
use std::convert::Infallible;
use std::time::Duration;

// This test is flaky. The error message will end with something like "0.999 B/s was observed"
#[should_panic = "minimum throughput was specified at 2 B/s, but throughput of"]
#[tokio::test]
async fn test_throughput_timeout_happens_for_slow_stream() {
    tracing_subscriber::fmt::init();
    tracing::info!("tracing is working");
    // let test_start_time = std::time::Instant::now();
    // Have to return results b/c `hyper::body::Body::wrap_stream` expects them
    let stream: FnStream<Result<String, Infallible>, _> = FnStream::new(|tx| {
        let async_sleep = aws_smithy_async::rt::sleep::TokioSleep::new();
        Box::pin(async move {
            for i in 0..10 {
                // Will send slightly less that 1 byte per second because ASCII digits have a size
                // of 1 byte and we sleep for 1 second after every digit we send.
                tx.send(Ok(format!("{}", i))).await.expect("failed to send");
                async_sleep.sleep(Duration::from_secs(1)).await;
            }
        })
    });
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
    let body = body.map(|body| {
        // Throw an error if the stream sends less than 2 bytes per second at any point
        let minimum_throughput = (2u64, Duration::from_secs(1));
        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(body, minimum_throughput),
        ))
    });
    let res = body.collect().await;

    match res {
        Ok(_) => panic!("Expected an error due to slow stream but no error occurred."),
        Err(e) => panic!("{}", e),
    }
}

#[tokio::test]
async fn test_throughput_timeout_doesnt_happen_for_fast_stream() {
    // Have to return results b/c `hyper::body::Body::wrap_stream` expects them
    let stream: FnStream<Result<String, Infallible>, _> = FnStream::new(|tx| {
        let async_sleep = aws_smithy_async::rt::sleep::TokioSleep::new();
        Box::pin(async move {
            for i in 0..10 {
                // Will send slightly less that 1 byte per millisecond because ASCII digits have a
                // size of 1 byte and we sleep for 1 millisecond after every digit we send.
                tx.send(Ok(format!("{}", i))).await.expect("failed to send");
                async_sleep.sleep(Duration::from_millis(1)).await;
            }
        })
    });
    let body = ByteStream::new(SdkBody::from(hyper::body::Body::wrap_stream(stream)));
    let body = body.map(|body| {
        // Throw an error if the stream sends less than 1 bytes per 5ms at any point
        let minimum_throughput = (1u64, Duration::from_millis(5));

        SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            MinimumThroughputBody::new(body, minimum_throughput),
        ))
    });
    let _res = body.collect().await.unwrap();
}
