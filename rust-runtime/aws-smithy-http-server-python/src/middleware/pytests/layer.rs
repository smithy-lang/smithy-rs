use aws_smithy_http_server::body::to_boxed;
use aws_smithy_http_server::proto::rest_json_1::RestJson1;
use aws_smithy_http_server_python::{
    middleware::{PyMiddlewareHandler, PyMiddlewareLayer},
    PyResponse,
};
use http::{Request, Response};
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
    let py_handler = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def identity_middleware(request, next):
    return await next(request)
"#,
            "",
            "",
        )?;
        Ok::<_, PyErr>(module.getattr("identity_middleware")?.into())
    })?;
    let handler = PyMiddlewareHandler {
        func: py_handler,
        name: "identity_middleware".to_string(),
        is_coroutine: true,
    };

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
    let py_handler = Python::with_gil(|py| {
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
        Ok::<_, PyErr>(locals.get_item("middleware").unwrap().into())
    })?;
    let handler = PyMiddlewareHandler {
        func: py_handler,
        name: "middleware".to_string(),
        is_coroutine: false,
    };

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
async fn nested_middlewares() -> PyResult<()> {
    let locals = Python::with_gil(|py| {
        Ok::<_, PyErr>(TaskLocals::new(pyo3_asyncio::tokio::get_current_loop(py)?))
    })?;
    let (first_py_handler, second_py_handler) = Python::with_gil(|py| {
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
        Ok::<_, PyErr>((
            locals.get_item("first_middleware").unwrap().into(),
            locals.get_item("second_middleware").unwrap().into(),
        ))
    })?;
    let first_handler = PyMiddlewareHandler {
        func: first_py_handler,
        name: "first_middleware".to_string(),
        is_coroutine: true,
    };
    let second_handler = PyMiddlewareHandler {
        func: second_py_handler,
        name: "second_middleware".to_string(),
        is_coroutine: false,
    };

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
