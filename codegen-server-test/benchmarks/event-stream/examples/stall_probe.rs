//! Duplex stall probe: client and server hyper HTTP/2 endpoints over an in-memory
//! byte stream, but on SEPARATE runtimes/threads, with a paced streaming request
//! body. Reproduces cross-thread wakeup latency absent from the Criterion bench.
use bytes::Bytes;
use event_stream_benchmark::{server, Case, Direction, Fixture, Implementation, Kind, Protocol};
use http::Request;
use http_body_util::{BodyExt, StreamBody};
use std::time::Instant;

use aws_smithy_http_server::body::Body;

struct ChannelStream(tokio::sync::mpsc::Receiver<Bytes>);
impl futures_util::Stream for ChannelStream {
    type Item = Result<http_body::Frame<Bytes>, std::convert::Infallible>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0
            .poll_recv(cx)
            .map(|opt| opt.map(|b| Ok(http_body::Frame::data(b))))
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let side = args.next().expect("side: legacy|schema");
    let count: usize = args.next().map(|v| v.parse().unwrap()).unwrap_or(1024);
    let requests: usize = args.next().map(|v| v.parse().unwrap()).unwrap_or(10);
    let budget: usize = args.next().map(|v| v.parse().unwrap()).unwrap_or(70 * 1024);
    let mode = args.next().unwrap_or_else(|| "duplex".into());
    let implementation = match side.as_str() {
        "legacy" => Implementation::Legacy,
        "schema" => Implementation::Schema,
        other => panic!("unknown side {other}"),
    };
    let case = Case {
        protocol: Protocol::RestJson,
        direction: Direction::Echo,
        kind: Kind::Structured,
        size: 1024,
        count,
    };
    let fixture = Fixture::new(case);
    let svc = server(implementation, case);
    enum Io {
        Duplex(tokio::io::DuplexStream),
        Tcp(std::net::TcpStream, bool),
    }
    let (server_half, client_half): (Io, Io) = match mode.as_str() {
        "duplex" => {
            let (c, s) = tokio::io::duplex(64 * 1024);
            (Io::Duplex(s), Io::Duplex(c))
        }
        "tcp" | "tcp-nodelay" => {
            let nodelay = mode == "tcp-nodelay";
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let join = std::thread::spawn(move || listener.accept().unwrap().0);
            let client = std::net::TcpStream::connect(addr).unwrap();
            let server = join.join().unwrap();
            client.set_nonblocking(true).unwrap();
            server.set_nonblocking(true).unwrap();
            (Io::Tcp(server, nodelay), Io::Tcp(client, nodelay))
        }
        other => panic!("unknown mode {other}"),
    };

    let server_thread = std::thread::spawn(move || {
        use hyper_util::rt::{TokioExecutor, TokioIo};
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            use tower::ServiceExt;
            let service =
                hyper::service::service_fn(move |request: Request<hyper::body::Incoming>| {
                    let svc = svc.clone();
                    async move { svc.oneshot(request.map(Body::new)).await }
                });
            let builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
            match server_half {
                Io::Duplex(io) => {
                    let _ = builder.serve_connection(TokioIo::new(io), service).await;
                }
                Io::Tcp(io, nodelay) => {
                    let io = tokio::net::TcpStream::from_std(io).unwrap();
                    io.set_nodelay(nodelay).unwrap();
                    let _ = builder.serve_connection(TokioIo::new(io), service).await;
                }
            }
        });
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async move {
        use hyper_util::rt::{TokioExecutor, TokioIo};
        // Extract template headers and the pre-encoded event chunks from the fixture.
        let (parts, body) = fixture.request().into_parts();
        let mut chunks: Vec<Bytes> = Vec::new();
        let mut body = body;
        while let Some(frame) = body.frame().await {
            chunks.push(frame.unwrap().into_data().unwrap());
        }
        let mut sender = match client_half {
            Io::Duplex(io) => {
                let (sender, connection) = hyper::client::conn::http2::handshake::<_, _, Body>(
                    TokioExecutor::new(),
                    TokioIo::new(io),
                )
                .await
                .unwrap();
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                sender
            }
            Io::Tcp(io, nodelay) => {
                let io = tokio::net::TcpStream::from_std(io).unwrap();
                io.set_nodelay(nodelay).unwrap();
                let (sender, connection) = hyper::client::conn::http2::handshake::<_, _, Body>(
                    TokioExecutor::new(),
                    TokioIo::new(io),
                )
                .await
                .unwrap();
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                sender
            }
        };

        let mut gaps: Vec<u64> = Vec::new();
        let started = Instant::now();
        for _ in 0..requests {
            let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(chunks.len().max(1));
            let mut builder = Request::builder()
                .method(parts.method.clone())
                .uri(parts.uri.clone());
            *builder.headers_mut().unwrap() = parts.headers.clone();
            let request = builder
                .body(Body::new(StreamBody::new(ChannelStream(rx))))
                .unwrap();
            let mut sent = 0usize;
            let mut sent_bytes = 0usize;
            let mut received_bytes = 0usize;
            let mut tx = Some(tx);
            // Prefill up to the in-flight byte budget.
            while sent < chunks.len() && sent_bytes + chunks[sent].len() <= budget {
                sent_bytes += chunks[sent].len();
                tx.as_ref().unwrap().send(chunks[sent].clone()).await.unwrap();
                sent += 1;
            }
            if sent == chunks.len() {
                tx = None;
            }
            sender.ready().await.unwrap();
            let response = sender.send_request(request).await.unwrap();
            assert_eq!(response.status(), 200);
            let mut rbody = response.into_body();
            let mut last = Instant::now();
            while let Some(frame) = rbody.frame().await {
                let now = Instant::now();
                gaps.push(now.duration_since(last).as_nanos() as u64);
                last = now;
                if let Ok(data) = frame.unwrap().into_data() {
                    received_bytes += data.len();
                }
                // Pace: keep at most `budget` bytes in flight.
                while sent < chunks.len() && sent_bytes - received_bytes.min(sent_bytes) + chunks[sent].len() <= budget {
                    sent_bytes += chunks[sent].len();
                    tx.as_ref().unwrap().send(chunks[sent].clone()).await.unwrap();
                    sent += 1;
                }
                if sent == chunks.len() {
                    tx = None;
                }
            }
            assert_eq!(sent, chunks.len());
        }
        let elapsed = started.elapsed();
        gaps.sort_unstable();
        let pct = |p: f64| gaps[((gaps.len() as f64 - 1.0) * p) as usize] as f64 / 1000.0;
        let total_events = count * requests;
        println!(
            "{{\"side\":\"{side}\",\"mode\":\"{mode}\",\"count\":{count},\"requests\":{requests},\"budget\":{budget},\"events_per_second\":{:.0},\"gap_p50_us\":{:.1},\"gap_p95_us\":{:.1},\"gap_p99_us\":{:.1},\"gap_max_us\":{:.1},\"frames\":{}}}",
            total_events as f64 / elapsed.as_secs_f64(),
            pct(0.50),
            pct(0.95),
            pct(0.99),
            *gaps.last().unwrap() as f64 / 1000.0,
            gaps.len(),
        );
    });
    drop(rt);
    let _ = server_thread;
    std::process::exit(0);
}
