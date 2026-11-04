/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use request_limits_server_sdk::{
    error, input, output, RequestLimitsService, RequestLimitsServiceConfig,
};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::{timeout, Instant},
};

struct TestServer {
    addr: SocketAddr,
    task: JoinHandle<std::io::Result<()>>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    async fn start() -> Self {
        let config = RequestLimitsServiceConfig::builder().build();
        let app = RequestLimitsService::builder(config)
            .echo(echo)
            .echo_with_override(echo_with_override)
            .upload(upload)
            .build()
            .expect("all test handlers are registered");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            request_limits_server_sdk::serve(listener, app.into_make_service()).await
        });
        Self { addr, task }
    }

    async fn request(&self, path: &str, framing: &str, body: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        let headers = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: {}\r\n{framing}\r\nConnection: close\r\n\r\n",
            if path == "/upload" { "application/octet-stream" } else { "application/json" }
        );
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        let mut response = Vec::new();
        timeout(Duration::from_secs(15), stream.read_to_end(&mut response))
            .await
            .expect("response deadline")
            .expect("read response");
        response
    }
}

async fn echo(input: input::EchoInput) -> Result<output::EchoOutput, error::EchoError> {
    Ok(output::EchoOutput { note: input.note })
}

async fn echo_with_override(
    input: input::EchoWithOverrideInput,
) -> Result<output::EchoWithOverrideOutput, error::EchoWithOverrideError> {
    Ok(output::EchoWithOverrideOutput { note: input.note })
}

async fn upload(input: input::UploadInput) -> Result<output::UploadOutput, error::UploadError> {
    Ok(output::UploadOutput {
        size: input.data.as_ref().len() as i64,
    })
}

fn assert_status(response: &[u8], status: u16) {
    let response = String::from_utf8_lossy(response);
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status} ")),
        "{response}"
    );
}

#[tokio::test]
async fn generated_serve_uses_json_header_deadline() {
    let server = TestServer::start().await;
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    let started = Instant::now();
    stream
        .write_all(b"POST /echo HTTP/1.1\r\nHost: localhost\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(15), stream.read_to_end(&mut response))
        .await
        .expect("JSON header deadline should close the connection")
        .unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(9750), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(13), "{elapsed:?}");
    assert!(response.is_empty() || response.starts_with(b"HTTP/1.1 408 "));
}

#[tokio::test]
async fn default_override_and_payload_deadlines_coexist_on_one_server() {
    let server = TestServer::start().await;
    let check = |path: &'static str, seconds: u64| {
        let server = &server;
        async move {
            let started = Instant::now();
            // Claim a longer body, then keep the connection open without finishing it.
            let response = server.request(path, "Content-Length: 100", b"{").await;
            let elapsed = started.elapsed();
            assert_status(&response, 408);
            assert!(
                elapsed >= Duration::from_millis(seconds * 1000 - 250),
                "{path}: {elapsed:?}"
            );
            assert!(
                elapsed < Duration::from_secs(seconds + 3),
                "{path}: {elapsed:?}"
            );
        }
    };
    tokio::join!(
        check("/echo", 5),
        check("/echo-override", 3),
        check("/upload", 10)
    );
}

#[tokio::test]
async fn json_and_payload_limits_accept_boundary_and_reject_larger_bodies() {
    let server = TestServer::start().await;
    for path in ["/echo", "/echo-override", "/upload"] {
        for (length, status) in [(1024, 200), (1025, 400)] {
            let body = if path == "/upload" {
                vec![b'x'; length]
            } else {
                format!("{{\"note\":\"{}\"}}", "x".repeat(length - 11)).into_bytes()
            };
            assert_eq!(body.len(), length);
            let response = server
                .request(path, &format!("Content-Length: {length}"), &body)
                .await;
            assert_status(&response, status);
            if path == "/upload" && status == 200 {
                assert!(String::from_utf8_lossy(&response).contains("\"size\":1024"));
            }
        }
        // No Content-Length: enforce the limit across multiple chunks of valid input.
        for (length, status) in [(1024, 200), (1025, 400)] {
            let body = if path == "/upload" {
                "x".repeat(length)
            } else {
                format!("{{\"note\":\"{}\"}}", "x".repeat(length - 11))
            };
            let chunks = format!(
                "200\r\n{}\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                &body[..512],
                length - 512,
                &body[512..]
            );
            let response = server
                .request(path, "Transfer-Encoding: chunked", chunks.as_bytes())
                .await;
            assert_status(&response, status);
        }
    }
}
