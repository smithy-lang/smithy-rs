/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::socket::TsSocket;
use aws_smithy_http_server::routing::IntoMakeService;
use tokio::runtime::Handle;

pub fn start_hyper_worker(
    socket: &TsSocket,
    app: tower::util::BoxCloneService<
        http::Request<aws_smithy_http_server::body::Body>,
        http::Response<aws_smithy_http_server::body::BoxBody>,
        std::convert::Infallible,
    >,
) -> napi::Result<()> {
    let server = hyper::Server::from_tcp(
        socket
            .to_raw_socket()?
            .try_into()
            .expect("Unable to convert socket2::Socket into std::net::TcpListener"),
    )
    .expect("Unable to create hyper server from shared socket")
    .serve(IntoMakeService::new(app));

    // TODO(fill-me-with-an-issue) albepose@ Fix this with Daniele
    let handle = Handle::current();
    handle.spawn(async move {
        // Process each socket concurrently.
        server.await
    });

    Ok(())
}
