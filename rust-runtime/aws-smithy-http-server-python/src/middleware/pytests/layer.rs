/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;

use aws_smithy_legacy_http_server::{
    body::{to_boxed, Body, BoxBody},
    protocol::rest_json_1::RestJson1,
};
use aws_smithy_legacy_http_server_python::{
    middleware::{PyMiddlewareHandler, PyMiddlewareLayer},
    PyMiddlewareException, PyResponse,
};
use http::{Request, Response, StatusCode};
use pretty_assertions::assert_eq;
use pyo3::{prelude::*, types::PyDict};
use pyo3_asyncio::TaskLocals;
use tokio_test::assert_ready_ok;
use tower::{layer::util::Stack, util::BoxCloneService, Layer, Service, ServiceExt};
use tower_test::mock;

#[pyo3_asyncio::tokio::test]
async fn identity_middleware() -> PyResult<()> {
    let layer = layer(
        r#"
async def middleware(request, next):
    return await next(request)
"#,
    );
    let (mut service, mut handle) = spawn_service(layer);

    let th = tokio::spawn(async move {
        let (req, send_response) = handle.next_request().await.unwrap();
        let req_body = hyper::body::to_bytes(req.into_body()).await.unwrap();
        assert_eq!(req_body, "hello server");
        send_response.send_response(
            Response::builder()
                .body(to_boxed("hello client"))
                .expect("could not create response"),
        );
    });

    let request = simple_request("hello server");
    let response = service.call(request);
    assert_body(response.await?, "hello client").await;

    th.await.unwrap();
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn returning_response_from_python_middleware() -> PyResult<()> {
    let layer = layer(
        r#"
def middleware(request, next):
    return Response(200, {}, b"hello client from Python")
"#,
    );
    let (mut service, _handle) = spawn_service(layer);

    let request = simple_request("hello server");
    let response = service.call(request);
    assert_body(response.await?, "hello client from Python").await;

    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn convert_exception_from_middleware_to_protocol_specific_response() -> PyResult<()> {
    let layer = layer(
        r#"
def middleware(request, next):
    raise RuntimeError("fail")
"#,
    );
    let (mut service, _handle) = spawn_service(layer);

    let request = simple_request("hello server");
    let response = service.call(request);
    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_body(response, r#"{"message":"RuntimeError: fail"}"#).await;

    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn uses_status_code_and_message_from_middleware_exception() -> PyResult<()> {
    let layer = layer(
        r#"
def middleware(request, next):
    raise MiddlewareException("access denied", 401)
"#,
    );
    let (mut service, _handle) = spawn_service(layer);

    let request = simple_request("hello server");
    let response = service.call(request);
    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_body(response, r#"{"message":"access denied"}"#).await;

    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn nested_middlewares() -> PyResult<()> {
    let first_layer = layer(
        r#"
async def middleware(request, next):
    return await next(request)
"#,
    );
    let second_layer = layer(
        r#"
def middleware(request, next):
    return Response(200, {}, b"hello client from Python second middleware")
"#,
    );
    let layer = Stack::new(first_layer, second_layer);
    let (mut service, _handle) = spawn_service(layer);

    let request = simple_request("hello server");
    let response = service.call(request);
    assert_body(
        response.await?,
        "hello client from Python second middleware",
    )
    .await;

    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn changes_request() -> PyResult<()> {
    let layer = layer(
        r#"
async def middleware(request, next):
    body = bytes(await request.body).decode()
    body_reversed = body[::-1]
    request.body = body_reversed.encode()
    # Add a new header
    request.headers["X-From-Middleware"] = "yes"
    # Change an existing header.
    request.headers["X-Existing"] = "changed"
    # Delete an existing header.
    del request.headers["X-To-Delete"]
    # Change the URI.
    request.uri = "/changed_uri"
    return await next(request)
"#,
    );
    let (mut service, mut handle) = spawn_service(layer);

    let th = tokio::spawn(async move {
        let (req, send_response) = handle.next_request().await.unwrap();
        assert_eq!("yes", req.headers().get("X-From-Middleware").unwrap());
        assert_eq!("changed", req.headers().get("X-Existing").unwrap());
        assert!(req.headers().get("X-To-Delete").is_none());
        assert_eq!("/changed_uri", req.uri());
        let req_body = hyper::body::to_bytes(req.into_body()).await.unwrap();
        assert_eq!(req_body, "hello server".chars().rev().collect::<String>());
        send_response.send_response(
            Response::builder()
                .body(to_boxed("hello client"))
                .expect("could not create response"),
        );
    });

    let mut request = simple_request("hello server");
    assert_ne!(request.uri(), "/changed_uri");
    // Add a header that the middleware should modify.
    let headers_mut = request.headers_mut();
    headers_mut.insert("X-Existing", http::HeaderValue::from_static("yes"));
    // Add a header that the middleware should remove.
    headers_mut.insert("X-To-Delete", http::HeaderValue::from_static("delete-this"));
    let response = service.call(request);
    assert_body(response.await?, "hello client").await;

    th.await.unwrap();
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn changes_response() -> PyResult<()> {
    let layer = layer(
        r#"
async def middleware(request, next):
    response = await next(request)
    body = bytes(await response.body).decode()
    body_reversed = body[::-1]
    response.body = body_reversed.encode()
    response.headers["X-From-Middleware"] = "yes"
    return response
"#,
    );
    let (mut service, mut handle) = spawn_service(layer);

    let th = tokio::spawn(async move {
        let (req, send_response) = handle.next_request().await.unwrap();
        let req_body = hyper::body::to_bytes(req.into_body()).await.unwrap();
        assert_eq!(req_body, "hello server");
        send_response.send_response(
            Response::builder()
                .body(to_boxed("hello client"))
                .expect("could not create response"),
        );
    });

    let request = simple_request("hello server");
    let response = service.call(request);
    let response = response.await.unwrap();
    assert_eq!(response.headers().get("X-From-Middleware").unwrap(), &"yes");
    assert_body(response, &"hello client".chars().rev().collect::<String>()).await;

    th.await.unwrap();
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn fails_if_req_is_used_after_calling_next() -> PyResult<()> {
    let layer = layer(
        r#"
async def middleware(request, next):
    uri = request.uri
    response = await next(request)
    uri = request.uri # <- fails
    return response
"#,
    );

    let (mut service, mut handle) = spawn_service(layer);

    let th = tokio::spawn(async move {
        let (req, send_response) = handle.next_request().await.unwrap();
        let req_body = hyper::body::to_bytes(req.into_body()).await.unwrap();
        assert_eq!(req_body, "hello server");
        send_response.send_response(
            Response::builder()
                .body(to_boxed("hello client"))
                .expect("could not create response"),
        );
    });

    let request = simple_request("hello server");
    let response = service.call(request);
    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_body(
        response,
        r#"{"message":"RuntimeError: request is accessed after `next` is called"}"#,
    )
    .await;

    th.await.unwrap();
    Ok(())
}

async fn assert_body(response: Response<BoxBody>, eq: &str) {
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, eq);
}

fn simple_request(body: &'static str) -> Request<Body> {
    Request::builder()
        .body(Body::from(body))
        .expect("could not create request")
}

#[allow(clippy::type_complexity)]
fn spawn_service<L, E>(
    layer: L,
) -> (
    mock::Spawn<L::Service>,
    mock::Handle<Request<Body>, Response<BoxBody>>,
)
where
    L: Layer<BoxCloneService<Request<Body>, Response<BoxBody>, Infallible>>,
    L::Service: Service<Request<Body>, Error = E>,
    E: std::fmt::Debug,
{
    let (mut service, handle) = mock::spawn_with(|svc| {
        let svc = svc
            .map_err(|err| panic!("service failed: {err}"))
            .boxed_clone();
        layer.layer(svc)
    });
    assert_ready_ok!(service.poll_ready());
    (service, handle)
}

fn layer(code: &str) -> PyMiddlewareLayer<RestJson1> {
    PyMiddlewareLayer::<RestJson1>::new(py_handler(code), task_locals())
}

fn task_locals() -> TaskLocals {
    Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })
    .unwrap()
}

fn py_handler(code: &str) -> PyMiddlewareHandler {
    Python::with_gil(|py| {
        // `py.run` under the hood uses `eval` (`PyEval_EvalCode` in C API)
        // and by default if you pass a `global` object without `__builtins__` key
        // it inserts `__builtins__` with reference to the `builtins` module
        // which provides prelude for Python so you can access `print()`, `bytes()`, `len()` etc.
        // but this is not working for Python 3.7.10 which is the version we are using in our CI
        // so our tests are failing in CI because there is no `print()`, `bytes()` etc.
        // in order to fix that we are manually extending `__main__` module to preserve `__builtins__`.
        let globals = PyModule::import(py, "__main__")?.dict();
        globals.set_item(
            "MiddlewareException",
            py.get_type::<PyMiddlewareException>(),
        )?;
        globals.set_item("Response", py.get_type::<PyResponse>())?;
        let locals = PyDict::new(py);
        py.run(code, Some(globals), Some(locals))?;
        let handler = locals
            .get_item("middleware")
            .expect("Python exception occurred during dictionary lookup")
            .expect("your handler must be named `middleware`")
            .into();
        PyMiddlewareHandler::new(py, handler)
    })
    .unwrap()
}
