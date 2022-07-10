/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::convert::Infallible;

use aws_smithy_logging::{HeaderMarker, InstrumentOperation, Sensitivity};
use http::{Request, Response};
use tower::util::service_fn;
use tower_service::Service;
use tracing_subscriber::EnvFilter;

async fn service(_: Request<()>) -> Result<Response<()>, Infallible> {
    let response = Response::builder()
        .status(400)
        .header("header-name-a", "visible")
        .header("header-name-b", "hidden")
        .header("prefix-hidden", "visible")
        .body(())
        .unwrap();
    Ok(response)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(EnvFilter::new("debug"))
        .init();

    let request = Request::get("http://localhost/a/b/c/d?bar=hidden")
        .header("header-name-a", "hidden")
        .body(())
        .unwrap();

    let svc = service_fn(service);

    let sensitivity = Sensitivity::new()
        .request_header(|name| HeaderMarker {
            value: name == "header-name-a",
            key_suffix: None,
        })
        .path(|index| index % 2 == 0)
        .query_key(|name| name == "bar")
        .response_header(|name| {
            if name.as_str().starts_with("prefix-") {
                HeaderMarker {
                    value: true,
                    key_suffix: Some("prefix-".len()),
                }
            } else {
                HeaderMarker {
                    value: name == "header-name-b",
                    key_suffix: None,
                }
            }
        })
        .status_code();
    let mut svc = InstrumentOperation::new(svc, "foo-operation").sensitivity(sensitivity);

    let _ = svc.call(request).await.unwrap();

    loop {}
}
