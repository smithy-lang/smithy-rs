/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::io;

use futures::StreamExt;
use futures_util::stream;
use hyper::Body;
use pyo3::{prelude::*, py_run};

use aws_smithy_http_server_python::types::ByteStream;
use aws_smithy_types::body::SdkBody;

#[pyo3_asyncio::tokio::test]
fn consuming_stream_on_python_synchronously() -> PyResult<()> {
    let bytestream = streaming_bytestream_from_vec(vec!["hello", " ", "world"]);
    Python::with_gil(|py| {
        let bytestream = bytestream.into_py(py);
        py_run!(
            py,
            bytestream,
            r#"
assert next(bytestream) == b"hello"
assert next(bytestream) == b" "
assert next(bytestream) == b"world"

try:
    next(bytestream)
    assert False, "iteration should stop by now"
except StopIteration:
    pass
"#
        );
        Ok(())
    })
}

#[pyo3_asyncio::tokio::test]
fn consuming_stream_on_python_synchronously_with_loop() -> PyResult<()> {
    let bytestream = streaming_bytestream_from_vec(vec!["hello", " ", "world"]);
    Python::with_gil(|py| {
        let bytestream = bytestream.into_py(py);
        py_run!(
            py,
            bytestream,
            r#"
total = []
for chunk in bytestream:
    total.append(chunk)

assert total == [b"hello", b" ", b"world"]
"#
        );
        Ok(())
    })
}

#[pyo3_asyncio::tokio::test]
fn consuming_stream_on_python_asynchronously() -> PyResult<()> {
    let bytestream = streaming_bytestream_from_vec(vec!["hello", " ", "world"]);
    Python::with_gil(|py| {
        let bytestream = bytestream.into_py(py);
        py_run!(
            py,
            bytestream,
            r#"
import asyncio

async def main(bytestream):
    assert await bytestream.__anext__() == b"hello"
    assert await bytestream.__anext__() == b" "
    assert await bytestream.__anext__() == b"world"

    try:
        await bytestream.__anext__()
        assert False, "iteration should stop by now"
    except StopAsyncIteration:
        pass

asyncio.run(main(bytestream))
"#
        );
        Ok(())
    })
}

#[pyo3_asyncio::tokio::test]
fn consuming_stream_on_python_asynchronously_with_loop() -> PyResult<()> {
    let bytestream = streaming_bytestream_from_vec(vec!["hello", " ", "world"]);
    Python::with_gil(|py| {
        let bytestream = bytestream.into_py(py);
        py_run!(
            py,
            bytestream,
            r#"
import asyncio

async def main(bytestream):
    total = []
    async for chunk in bytestream:
        total.append(chunk)
    assert total == [b"hello", b" ", b"world"]

asyncio.run(main(bytestream))
"#
        );
        Ok(())
    })
}

#[pyo3_asyncio::tokio::test]
async fn streaming_back_to_rust_from_python() -> PyResult<()> {
    let bytestream = streaming_bytestream_from_vec(vec!["hello", " ", "world"]);
    let py_stream = Python::with_gil(|py| {
        let module = PyModule::from_code(
            py,
            r#"
async def handler(bytestream):
    async for chunk in bytestream:
        yield "🐍 " + chunk.decode("utf-8")
    yield "Hello from Python!"
"#,
            "",
            "",
        )?;
        let handler = module.getattr("handler")?;
        let output = handler.call1((bytestream,))?;
        Ok::<_, PyErr>(pyo3_asyncio::tokio::into_stream_v2(output))
    })??;

    let mut py_stream = py_stream.map(|v| Python::with_gil(|py| v.extract::<String>(py).unwrap()));

    assert_eq!(py_stream.next().await, Some("🐍 hello".to_string()));
    assert_eq!(py_stream.next().await, Some("🐍  ".to_string()));
    assert_eq!(py_stream.next().await, Some("🐍 world".to_string()));
    assert_eq!(
        py_stream.next().await,
        Some("Hello from Python!".to_string())
    );
    assert_eq!(py_stream.next().await, None);

    Ok(())
}

fn streaming_bytestream_from_vec(chunks: Vec<&'static str>) -> ByteStream {
    let stream = stream::iter(chunks.into_iter().map(Ok::<_, io::Error>));
    let body = Body::wrap_stream(stream);
    ByteStream::new(SdkBody::from_body_0_4(body))
}
