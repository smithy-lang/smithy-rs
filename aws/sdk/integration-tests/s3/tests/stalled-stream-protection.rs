/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::{Region, StalledStreamProtectionConfig};
use aws_sdk_s3::{Client, Config};
use bytes::BytesMut;
use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::debug;

// #[tokio::test]
// async fn test_stalled_stream_protection_defaults_for_upload() {
//     // // We spawn a faulty server that will close the connection after
//     // // writing half of the response body.
//     // let (server, server_addr) = start_faulty_server().await;
//     // let _ = tokio::spawn(server);
//
//     let conf = Config::builder()
//         .with_test_defaults()
//         .region(Region::new("us-east-1"))
//         // .endpoint_url(format!("http://{server_addr}"))
//         .stalled_stream_protection_config(
//             StalledStreamProtectionConfig::new_enabled()
//                 .build()
//         )
//         .build();
//     let client = Client::from_conf(conf);
//
//     let err = client
//         .put_object()
//         .bucket("zhessler-test-bucket")
//         .key("stalled-stream-test.txt")
//         .body(ByteStream::from_static(b"Hello"))
//         .send()
//         .await
//         .expect_err("upload stream stalled out");
//
//     // assert_eq!(err.kind(), aws_smithy_types::error::Kind::Service);
//     assert_eq!(DisplayErrorContext(err).to_string(), "Stream stalled out");
// }

#[tokio::test]
async fn test_stalled_stream_protection_defaults_for_download() {
    let _ = tracing_subscriber::fmt::try_init();

    // We spawn a faulty server that will close the connection after
    // writing half of the response body.
    let (server, server_addr) = start_faulty_server().await;
    let _ = tokio::spawn(server);

    let conf = Config::builder()
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://{server_addr}"))
        .stalled_stream_protection_config(StalledStreamProtectionConfig::new_enabled().build())
        .build();
    let client = Client::from_conf(conf);

    let res = client
        .get_object()
        .bucket("zhessler-test-bucket")
        .key("stalled-stream-test.txt")
        .send()
        .await
        .unwrap();

    let err = res
        .body
        .collect()
        .await
        .expect_err("download stream stalled out");
    assert_eq!(err.to_string(), "Stream stalled out");
}

async fn start_faulty_server() -> (impl Future<Output = ()>, SocketAddr) {
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::sleep;

    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .expect("socket is free");
    let bind_addr = listener.local_addr().unwrap();

    async fn process_socket(socket: TcpStream) {
        let mut buf = BytesMut::new();
        let response: &[u8] = br#"HTTP/1.1 200 OK
x-amz-request-id: 4B4NGF0EAWN0GE63
content-length: 12
etag: 3e25960a79dbc69b674cd4ec67a72c62
content-type: application/octet-stream
server: AmazonS3
content-encoding:
last-modified: Tue, 21 Jun 2022 16:29:14 GMT
date: Tue, 21 Jun 2022 16:29:23 GMT
x-amz-id-2: kPl+IVVZAwsN8ePUyQJZ40WD9dzaqtr4eNESArqE68GSKtVvuvCTDe+SxhTT+JTUqXB1HL4OxNM=
accept-ranges: bytes

Hello"#;
        let mut time_to_respond = false;

        loop {
            match socket.try_read_buf(&mut buf) {
                Ok(0) => {
                    unreachable!(
                        "The connection will be closed before this branch is ever reached"
                    );
                }
                Ok(n) => {
                    debug!("read {n} bytes from the socket");

                    // Check for CRLF to see if we've received the entire HTTP request.
                    if buf.ends_with(b"\r\n\r\n") {
                        time_to_respond = true;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    debug!("reading would block, sleeping for 1ms and then trying again");
                    sleep(Duration::from_millis(1)).await;
                }
                Err(e) => {
                    panic!("{e}")
                }
            }

            if socket.writable().await.is_ok() && time_to_respond {
                // The content length is 12 but we'll only write 5 bytes
                socket.try_write(response).unwrap();
                // We break from the R/W loop after sending a partial response in order to
                // close the connection early.
                debug!("faulty server has written partial response, now getting stuck");
                break;
            }
        }

        loop {
            // do nothing to simulate being stuck.
        }
    }

    let fut = async move {
        loop {
            let (socket, addr) = listener
                .accept()
                .await
                .expect("listener can accept new connections");
            debug!("server received new connection from {addr:?}");
            let start = std::time::Instant::now();
            process_socket(socket).await;
            debug!(
                "connection to {addr:?} closed after {:.02?}",
                start.elapsed()
            );
        }
    };

    (fut, bind_addr)
}
