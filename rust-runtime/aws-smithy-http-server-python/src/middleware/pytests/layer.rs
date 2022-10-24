/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::{body::to_boxed, proto::rest_json_1::RestJson1};
use aws_smithy_http_server_python::{
    middleware::{PyMiddlewareHandler, PyMiddlewareLayer},
    PyMiddlewareException, PyResponse,
};
use http::{Request, Response, StatusCode};
use hyper::Body;
use pretty_assertions::assert_eq;
use pyo3::{
    prelude::*,
    types::{IntoPyDict, PyDict},
};
use pyo3_asyncio::TaskLocals;
use tokio_test::assert_ready_ok;
use tower::{layer::util::Stack, Layer, ServiceExt};
use tower_test::mock;

#[pyo3_asyncio::tokio::test]
async fn identity_middleware() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def identity_middleware(request, next):
    return await next(request)
"#,
            "",
            "",
        )?;
        let handler = module.getattr("identity_middleware")?.into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;
    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, mut handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

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

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let body = hyper::body::to_bytes(response.await.unwrap().into_body())
        .await
        .unwrap();
    assert_eq!(body, "hello client");
    th.await.unwrap();
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn returning_response_from_python_middleware() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let globals = [("Response", py.get_type::<PyResponse>())].into_py_dict(py);
        let locals = PyDict::new(py);
        py.run(
            r#"
def middleware(request, next):
    return Response(200, {}, b"hello client from Python")
"#,
            Some(globals),
            Some(locals),
        )?;
        let handler = locals.get_item("middleware").unwrap().into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;

    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, _handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let body = hyper::body::to_bytes(response.await.unwrap().into_body())
        .await
        .unwrap();
    assert_eq!(body, "hello client from Python");
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn convert_exception_from_middleware_to_protocol_specific_response() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let locals = PyDict::new(py);
        py.run(
            r#"
def middleware(request, next):
    raise RuntimeError("fail")
"#,
            None,
            Some(locals),
        )?;
        let handler = locals.get_item("middleware").unwrap().into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;

    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, _handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, r#"{"message":"RuntimeError: fail"}"#);
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn uses_status_code_and_message_from_middleware_exception() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let globals = [(
            "MiddlewareException",
            py.get_type::<PyMiddlewareException>(),
        )]
        .into_py_dict(py);
        let locals = PyDict::new(py);
        py.run(
            r#"
def middleware(request, next):
    raise MiddlewareException("access denied", 401)
"#,
            Some(globals),
            Some(locals),
        )?;
        let handler = locals.get_item("middleware").unwrap().into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;

    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, _handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, r#"{"message":"access denied"}"#);
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn nested_middlewares() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let (first_handler, second_handler) = Python::with_gil(|py| {
        let globals = [("Response", py.get_type::<PyResponse>())].into_py_dict(py);
        let locals = PyDict::new(py);

        py.run(
            r#"
async def first_middleware(request, next):
    return await next(request)

def second_middleware(request, next):
    return Response(200, {}, b"hello client from Python second middleware")
"#,
            Some(globals),
            Some(locals),
        )?;
        let first_handler =
            PyMiddlewareHandler::new(py, locals.get_item("first_middleware").unwrap().into())?;
        let second_handler =
            PyMiddlewareHandler::new(py, locals.get_item("second_middleware").unwrap().into())?;
        Ok::<_, PyErr>((first_handler, second_handler))
    })?;

    let layer = Stack::new(
        PyMiddlewareLayer::<RestJson1>::new(second_handler, locals.clone()),
        PyMiddlewareLayer::<RestJson1>::new(first_handler, locals.clone()),
    );
    let (mut service, _handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let body = hyper::body::to_bytes(response.await.unwrap().into_body())
        .await
        .unwrap();
    assert_eq!(body, "hello client from Python second middleware");
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn changes_request() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def middleware(request, next):
    body = bytes(await request.body).decode()
    body_reversed = body[::-1]
    request.body = body_reversed.encode()
    request.set_header("X-From-Middleware", "yes")
    return await next(request)
"#,
            "",
            "",
        )?;
        let handler = module.getattr("middleware")?.into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;
    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, mut handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

    let th = tokio::spawn(async move {
        let (req, send_response) = handle.next_request().await.unwrap();
        assert_eq!(&"yes", req.headers().get("X-From-Middleware").unwrap());
        let req_body = hyper::body::to_bytes(req.into_body()).await.unwrap();
        assert_eq!(req_body, "hello server".chars().rev().collect::<String>());
        send_response.send_response(
            Response::builder()
                .body(to_boxed("hello client"))
                .expect("could not create response"),
        );
    });

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let body = hyper::body::to_bytes(response.await.unwrap().into_body())
        .await
        .unwrap();
    assert_eq!(body, "hello client");
    th.await.unwrap();
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn changes_response() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def middleware(request, next):
    response = await next(request)
    body = bytes(await response.body).decode()
    body_reversed = body[::-1]
    response.body = body_reversed.encode()
    response.set_header("X-From-Middleware", "yes")
    return response
"#,
            "",
            "",
        )?;
        let handler = module.getattr("middleware")?.into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;
    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, mut handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

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

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let response = response.await.unwrap();
    assert_eq!(&"yes", response.headers().get("X-From-Middleware").unwrap());

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, "hello client".chars().rev().collect::<String>());
    th.await.unwrap();
    Ok(())
}

#[pyo3_asyncio::tokio::test]
async fn fails_if_req_is_used_after_calling_next() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let handler = Python::with_gil(|py| {
        let locals = PyDict::new(py);
        py.run(
            r#"
async def middleware(request, next):
    uri = request.uri
    response = await next(request)
    uri = request.uri # <- fails
    return response
"#,
            None,
            Some(locals),
        )?;
        let handler = locals.get_item("middleware").unwrap().into();
        Ok::<_, PyErr>(PyMiddlewareHandler::new(py, handler)?)
    })?;

    let layer = PyMiddlewareLayer::<RestJson1>::new(handler, locals);
    let (mut service, mut handle) = mock::spawn_with(|svc| {
        let svc = svc.map_err(|err| panic!("service failed: {err}"));
        let svc = layer.layer(svc);
        svc
    });
    assert_ready_ok!(service.poll_ready());

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

    let request = Request::builder()
        .body(Body::from("hello server"))
        .expect("could not create request");
    let response = service.call(request);

    let response = response.await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    assert_eq!(body, r#"{"message":"RuntimeError: request is gone"}"#);
    th.await.unwrap();
    Ok(())
}
