/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Streaming blob inputs with deferred validation of the completed input.
mod cbor;

use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use aws_smithy_runtime_api::http::Request;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{byte_stream::ByteStream, Blob};
use bytes::{Buf, Bytes, BytesMut};
use http_body::{Body, Frame};

use super::{DeserializeError, RequestBodyCollectionConfig, SelectedOperation};
use crate::body::{RequestBody, DEFAULT_FRAME_BUDGET, DEFAULT_STREAMING_CHUNK_BUDGET};
use cbor::{error, header, Scanned, Scanner};

type Visitor<'a> = dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError> + 'a;
type Decode = dyn Fn(&[u8], &mut Visitor<'_>) -> Result<(), DeserializeError> + Send + Sync;

/// An owned protocol decoding session. Generated glue supplies the input's ordinary-field builder.
///
/// The session owns the transport and codec configuration. Borrowed deserializers exist only
/// while processing a complete ordinary value, and never cross an asynchronous body read.
pub struct StreamingRequestDecoder {
    state: Arc<Mutex<State>>,
    decode: Box<Decode>,
}

impl fmt::Debug for StreamingRequestDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingRequestDecoder").finish_non_exhaustive()
    }
}

impl StreamingRequestDecoder {
    pub(crate) fn rest<C: Codec + Send + Sync + 'static>(
        request: Request<RequestBody>,
        codec: Arc<C>,
    ) -> Result<Self, DeserializeError> {
        let (head, state) = State::new(request, None)?;
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            decode: Box::new(move |_, visitor| {
                let mut deserializer = super::request_bindings::RestRequestDeserializer::new(
                    &*codec,
                    head.uri_ref(),
                    head.headers(),
                    head.body(),
                );
                visitor(&mut deserializer).map_err(Into::into)
            }),
        })
    }

    pub(crate) fn cbor<C: Codec + Send + Sync + 'static>(
        member: &'static Schema<'static>,
        request: Request<RequestBody>,
        codec: Arc<C>,
    ) -> Result<Self, DeserializeError> {
        let name = member
            .member_name()
            .ok_or_else(|| error("streaming blob member has no name"))?;
        let (head, state) = State::new(request, Some(name))?;
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            decode: Box::new(move |bytes, visitor| {
                // Preserve the empty-request exemption. Any nonempty document (including
                // a map with no ordinary members) checks the actual Content-Type.
                if !bytes.is_empty() {
                    super::protocol::request::check_content_type(head.headers(), Some("application/cbor"), &[])?;
                }
                visitor(&mut codec.create_deserializer(bytes)).map_err(Into::into)
            }),
        })
    }
}

/// Errors while consuming or completing a streaming input.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StreamingInputError {
    /// Read the blob to EOF before calling `finish`; this error leaves the input usable.
    #[error("the streaming blob has not been read to EOF")]
    StreamNotFinished,
    /// A successful `finish` already returned the input.
    #[error("the streaming input was already finished")]
    AlreadyFinished,
    /// Collecting cannot recover bytes already delivered to the handler.
    #[error("the streaming blob has already been read")]
    AlreadyRead,
    /// A previous read or completion failed; the session is terminal.
    #[error("streaming input failed: {0}")]
    Failed(String),
    /// Decoding or modeled validation failed.
    #[error(transparent)]
    Deserialize(#[from] DeserializeError),
}

/// A live blob and the pending ordinary fields of an operation input.
///
/// `T` contains only the ordinary fields. Read [`data`](Self::data) to EOF, then call
/// [`finish`](Self::finish) to read trailing fields and validate the completed input.
/// Bytes may already have been processed when validation fails.
pub struct StreamingInput<T> {
    /// The decoded blob bytes, with normal asynchronous backpressure.
    pub data: ByteStream,
    decoder: StreamingRequestDecoder,
    fields: Box<dyn FnMut(Fields<'_>) -> Result<Option<T>, DeserializeError> + Send>,
    finished: bool,
}

enum Fields<'a> {
    Read(&'a mut dyn ShapeDeserializer),
    Finish(bool),
}

impl<T> fmt::Debug for StreamingInput<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingInput")
            .field("data_present", &self.is_present())
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl<T> StreamingInput<T> {
    /// Whether the modeled blob was present. An explicitly empty blob is present;
    /// an absent (or null) CBOR member is not. REST's raw payload is always present.
    pub fn is_present(&self) -> bool {
        self.decoder.state.lock().unwrap().present
    }

    /// Generated glue keeps its private builder behind callbacks, without exposing a builder
    /// associated type or changing the shared serde traits.
    #[doc(hidden)]
    pub async fn prepare<B: Send + 'static>(
        decoder: StreamingRequestDecoder,
        builder: B,
        read: fn(&mut B, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        finish: fn(B, bool) -> Result<T, DeserializeError>,
    ) -> Result<Self, DeserializeError>
    where
        T: 'static,
    {
        let state = decoder.state.clone();
        let mut builder = Some(builder);
        let mut input = Self {
            data: ByteStream::from_body_1_x(BlobBody(state)),
            decoder,
            fields: Box::new(move |action| match action {
                Fields::Read(deserializer) => {
                    read(builder.as_mut().expect("builder exists until finish"), deserializer)?;
                    Ok(None)
                }
                Fields::Finish(present) => finish(builder.take().expect("builder finished once"), present).map(Some),
            }),
            finished: false,
        };
        let timeout = input.decoder.state.lock().unwrap().config.read_timeout;
        let work = input.read_fields();
        match timeout {
            Some(timeout) => tokio::time::timeout(timeout, work)
                .await
                .map_err(|_| error("request body read timed out"))??,
            None => work.await?,
        }
        Ok(input)
    }

    async fn read_fields(&mut self) -> Result<(), DeserializeError> {
        loop {
            let value = poll_fn(|cx| self.decoder.state.lock().unwrap().poll_fields(cx)).await?;
            let Some(value) = value else { return Ok(()) };
            (self.decoder.decode)(
                &value,
                &mut |deserializer| match (self.fields)(Fields::Read(deserializer)) {
                    Ok(_) => Ok(()),
                    Err(DeserializeError::Serde(error)) => Err(error),
                    Err(error) => Err(SerdeError::custom(error.to_string())),
                },
            )?;
        }
    }

    /// Reads fields after the blob and validates the completed input.
    ///
    /// Calling this before blob EOF returns `StreamNotFinished` without draining it.
    /// Cancellation while reading trailing fields leaves the session resumable.
    ///
    /// A duplicate CBOR streaming member is rejected: bytes already delivered to the
    /// handler cannot be replaced with the later value. Errors after the handler starts
    /// are returned to the handler, which decides how to respond.
    pub async fn finish(&mut self) -> Result<T, StreamingInputError> {
        if self.finished {
            return Err(StreamingInputError::AlreadyFinished);
        }
        {
            let state = self.decoder.state.lock().unwrap();
            if let Some(error) = &state.failed {
                return Err(StreamingInputError::Failed(error.clone()));
            }
            if !state.blob_done {
                return Err(StreamingInputError::StreamNotFinished);
            }
        }
        let timeout = self.decoder.state.lock().unwrap().config.read_timeout;
        let work = self.read_fields();
        let result = match timeout {
            Some(timeout) => tokio::time::timeout(timeout, work)
                .await
                .unwrap_or_else(|_| Err(error("request body read timed out").into())),
            None => work.await,
        };
        if let Err(error) = result {
            self.decoder.state.lock().unwrap().fail(error.to_string());
            return Err(error.into());
        }
        let present = self.is_present();
        match (self.fields)(Fields::Finish(present)) {
            Ok(Some(input)) => {
                self.finished = true;
                Ok(input)
            }
            Ok(None) => unreachable!("finish callback returns the completed input"),
            Err(error) => {
                self.decoder.state.lock().unwrap().fail(error.to_string());
                Err(error.into())
            }
        }
    }

    /// Collects an unread blob, then reads trailing fields and validates `T`.
    ///
    /// The routed operation's byte limit counts the entire wire body, including framing and
    /// fields read during preparation. One timeout covers collection and completion together.
    /// `None` preserves an absent blob; `Some(Blob::new([]))` preserves an explicit empty one.
    pub async fn into_input_and_blob(mut self) -> Result<(T, Option<Blob>), StreamingInputError> {
        let timeout = {
            let mut state = self.decoder.state.lock().unwrap();
            if state.delivered != 0 {
                return Err(StreamingInputError::AlreadyRead);
            }
            state.collecting = true;
            let timeout = state.config.read_timeout;
            // The outer deadline includes finish; do not start another deadline there.
            state.config.read_timeout = None;
            state.check_limit().map_err(DeserializeError::from)?;
            timeout
        };
        let work = async {
            let mut bytes = Vec::new();
            while let Some(chunk) = self.data.next().await {
                let chunk = chunk.map_err(|err| StreamingInputError::Failed(err.to_string()))?;
                bytes.extend_from_slice(&chunk);
            }
            let blob = self.is_present().then(|| Blob::new(bytes));
            let input = self.finish().await?;
            Ok((input, blob))
        };
        match timeout {
            Some(timeout) => tokio::time::timeout(timeout, work)
                .await
                .map_err(|_| StreamingInputError::Failed("request body read timed out".into()))?,
            None => work.await,
        }
    }

    /// Collects and validates the entire input, returning only its optional blob.
    pub async fn into_blob(self) -> Result<Option<Blob>, StreamingInputError> {
        self.into_input_and_blob().await.map(|(_, blob)| blob)
    }
}

#[derive(Debug)]
struct State {
    body: RequestBody,
    buffer: BytesMut,
    config: RequestBodyCollectionConfig,
    wire_bytes: usize,
    delivered: u64,
    collecting: bool,
    polls: usize,
    work: usize,
    eof: bool,
    failed: Option<String>,
    name: Option<&'static str>,
    started: bool,
    remaining: Option<u64>,
    map_done: bool,
    key: Option<Bytes>,
    key_is_blob: bool,
    scanner: Scanner,
    present: bool,
    seen_blob: bool,
    blob_started: bool,
    blob_done: bool,
    blob_remaining: u64,
    indefinite_blob: bool,
}

impl State {
    fn new(
        request: Request<RequestBody>,
        name: Option<&'static str>,
    ) -> Result<(Request<Bytes>, Self), DeserializeError> {
        let config = request
            .extension::<SelectedOperation>()
            .ok_or_else(|| {
                DeserializeError::InternalFailure(crate::Error::new(error(
                    "selected operation missing while preparing streaming input",
                )))
            })?
            .request_body_config();
        let mut request = request.map(Some);
        let body = request.body_mut().take().expect("request body exists");
        Ok((
            request.map(|_| Bytes::new()),
            Self {
                body,
                buffer: BytesMut::new(),
                config,
                wire_bytes: 0,
                delivered: 0,
                collecting: false,
                polls: 0,
                work: 0,
                eof: false,
                failed: None,
                name,
                started: false,
                remaining: None,
                map_done: false,
                key: None,
                key_is_blob: false,
                scanner: Scanner::default(),
                present: name.is_none(),
                seen_blob: false,
                blob_started: false,
                blob_done: false,
                blob_remaining: 0,
                indefinite_blob: false,
            },
        ))
    }

    fn budget(&self) -> usize {
        if self.collecting {
            DEFAULT_FRAME_BUDGET.get()
        } else {
            DEFAULT_STREAMING_CHUNK_BUDGET.get()
        }
    }

    fn fail(&mut self, error: String) {
        self.failed = Some(error);
        self.body = RequestBody::from(Bytes::new());
        self.buffer.clear();
    }

    fn check_limit(&self) -> Result<(), SerdeError> {
        if self.collecting && self.config.max_bytes.is_some_and(|limit| self.wire_bytes > limit.get()) {
            return Err(error("request body exceeded the configured byte limit"));
        }
        Ok(())
    }

    fn cooperate(&mut self, cx: &mut Context<'_>) -> bool {
        self.work += 1;
        if self.work >= self.budget() {
            self.work = 0;
            cx.waker().wake_by_ref();
            true
        } else {
            false
        }
    }

    fn poll_more(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), SerdeError>> {
        if self.eof {
            return Poll::Ready(Err(error("truncated CBOR input")));
        }
        if self.polls >= self.budget() {
            self.polls = 0;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        match Pin::new(&mut self.body).poll_frame(cx) {
            Poll::Pending => {
                self.polls = 0;
                self.work = 0;
                Poll::Pending
            }
            Poll::Ready(frame) => {
                self.work = 0;
                self.polls += 1;
                match frame {
                    None => self.eof = true,
                    Some(Err(err)) => return Poll::Ready(Err(SerdeError::custom(err.to_string()))),
                    Some(Ok(frame)) => {
                        if let Ok(data) = frame.into_data() {
                            self.wire_bytes = self
                                .wire_bytes
                                .checked_add(data.len())
                                .ok_or_else(|| error("request body size overflow"))?;
                            self.check_limit()?;
                            self.buffer.extend_from_slice(&data);
                        }
                    }
                }
                Poll::Ready(Ok(()))
            }
        }
    }

    fn poll_header(&mut self, cx: &mut Context<'_>) -> Poll<Result<(u8, Option<u64>, usize), SerdeError>> {
        loop {
            if let Some(header) = header(&self.buffer)? {
                return Poll::Ready(Ok(header));
            }
            std::task::ready!(self.poll_more(cx))?;
        }
    }

    fn poll_value(&mut self, cx: &mut Context<'_>) -> Poll<Result<Bytes, SerdeError>> {
        loop {
            // Ordinary members are buffered; apply the configured bound to that buffer
            // even when the caller streams the blob instead of collecting it.
            match self.scanner.scan(&self.buffer, self.budget())? {
                Scanned::Complete(length) => {
                    if self.config.max_bytes.is_some_and(|limit| length > limit.get()) {
                        return Poll::Ready(Err(error("ordinary CBOR value exceeded the configured byte limit")));
                    }
                    self.scanner = Scanner::default();
                    return Poll::Ready(Ok(self.buffer.split_to(length).freeze()));
                }
                Scanned::More => {
                    if self
                        .config
                        .max_bytes
                        .is_some_and(|limit| self.buffer.len() > limit.get())
                    {
                        return Poll::Ready(Err(error("ordinary CBOR value exceeded the configured byte limit")));
                    }
                    std::task::ready!(self.poll_more(cx))?;
                }
                Scanned::Yield => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
            }
        }
    }

    fn poll_fields(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<Bytes>, SerdeError>> {
        if let Some(message) = &self.failed {
            return Poll::Ready(Err(error(message)));
        }
        if self.name.is_none() {
            if !self.started {
                self.started = true;
                self.blob_started = true;
                return Poll::Ready(Ok(Some(Bytes::new())));
            }
            return Poll::Ready(Ok(None));
        }
        loop {
            if self.cooperate(cx) {
                return Poll::Pending;
            }
            if self.blob_started && !self.blob_done {
                return Poll::Ready(Ok(None));
            }
            if !self.started {
                // Empty HTTP bodies are an empty input, as in the existing RPC codec.
                while self.buffer.is_empty() && !self.eof {
                    std::task::ready!(self.poll_more(cx))?;
                }
                if self.eof && self.buffer.is_empty() {
                    self.started = true;
                    self.map_done = true;
                    self.blob_done = true;
                    return Poll::Ready(Ok(Some(Bytes::new())));
                }
                let (major, length, width) = std::task::ready!(self.poll_header(cx))?;
                if major != 5 {
                    return Poll::Ready(Err(error("expected a CBOR input map")));
                }
                self.started = true;
                self.remaining = length;
                self.buffer.advance(width);
                // Let the configured codec validate media type and ordinary empty-map handling.
                return Poll::Ready(Ok(Some(Bytes::from_static(&[0xa0]))));
            }
            if self.map_done {
                if !self.buffer.is_empty() {
                    return Poll::Ready(Err(error("trailing bytes after CBOR value")));
                }
                if self.eof {
                    self.blob_done = true;
                    return Poll::Ready(Ok(None));
                }
                std::task::ready!(self.poll_more(cx))?;
                continue;
            }
            if self.key.is_none() {
                if self.remaining == Some(0) {
                    self.map_done = true;
                    continue;
                }
                let (major, value, _) = std::task::ready!(self.poll_header(cx))?;
                if major == 7 && value.is_none() && self.remaining.is_none() {
                    self.buffer.advance(1);
                    self.map_done = true;
                    continue;
                }
                let key = std::task::ready!(self.poll_value(cx))?;
                let mut decoder = aws_smithy_cbor::Decoder::new(&key);
                let name = decoder.str().map_err(|err| SerdeError::custom(err.to_string()))?;
                self.key_is_blob = Some(name.as_ref()) == self.name;
                self.key = Some(key);
            }
            if self.key_is_blob {
                if self.seen_blob {
                    return Poll::Ready(Err(error("duplicate streaming blob member")));
                }
                let (major, length, width) = std::task::ready!(self.poll_header(cx))?;
                if self.buffer[0] == 0xf6 {
                    // Null is an absent struct member, matching the strict CBOR codec.
                    self.seen_blob = true;
                    self.buffer.advance(width);
                    self.key.take();
                    if let Some(left) = &mut self.remaining {
                        *left -= 1;
                    }
                    continue;
                }
                if major != 2 {
                    return Poll::Ready(Err(error("expected a CBOR byte string")));
                }
                self.buffer.advance(width);
                self.present = true;
                self.seen_blob = true;
                self.blob_started = true;
                self.indefinite_blob = length.is_none();
                self.blob_remaining = length.unwrap_or(0);
                self.key.take();
                if let Some(left) = &mut self.remaining {
                    *left -= 1;
                }
                return Poll::Ready(Ok(None));
            }
            let value = std::task::ready!(self.poll_value(cx))?;
            let key = self.key.take().expect("key precedes value");
            if let Some(left) = &mut self.remaining {
                *left -= 1;
            }
            let mut document = Vec::with_capacity(2 + key.len() + value.len());
            document.push(0xbf);
            document.extend_from_slice(&key);
            document.extend_from_slice(&value);
            document.push(0xff);
            return Poll::Ready(Ok(Some(Bytes::from(document))));
        }
    }

    fn poll_blob(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, SerdeError>>> {
        if let Some(message) = &self.failed {
            return Poll::Ready(Some(Err(error(message))));
        }
        if self.blob_done {
            return Poll::Ready(None);
        }
        loop {
            if self.cooperate(cx) {
                return Poll::Pending;
            }
            if self.name.is_none() {
                if !self.buffer.is_empty() {
                    let bytes = self.buffer.split().freeze();
                    self.delivered += bytes.len() as u64;
                    return Poll::Ready(Some(Ok(Frame::data(bytes))));
                }
                if self.eof {
                    self.blob_done = true;
                    return Poll::Ready(None);
                }
                std::task::ready!(self.poll_more(cx))?;
                continue;
            }
            if self.blob_remaining != 0 {
                if self.buffer.is_empty() {
                    std::task::ready!(self.poll_more(cx))?;
                    continue;
                }
                let length = self.blob_remaining.min(self.buffer.len() as u64) as usize;
                let bytes = self.buffer.split_to(length).freeze();
                self.blob_remaining -= length as u64;
                self.delivered += length as u64;
                return Poll::Ready(Some(Ok(Frame::data(bytes))));
            }
            if !self.indefinite_blob {
                self.blob_done = true;
                return Poll::Ready(None);
            }
            let (major, length, width) = std::task::ready!(self.poll_header(cx))?;
            if major == 7 && length.is_none() {
                self.buffer.advance(width);
                self.blob_done = true;
                return Poll::Ready(None);
            }
            if major != 2 || length.is_none() {
                return Poll::Ready(Some(Err(error("invalid indefinite byte string chunk"))));
            }
            self.buffer.advance(width);
            self.blob_remaining = length.unwrap();
        }
    }
}

#[derive(Debug)]
struct BlobBody(Arc<Mutex<State>>);
impl Body for BlobBody {
    type Data = Bytes;
    type Error = crate::Error;
    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let mut state = self.0.lock().unwrap();
        if state.failed.is_some() {
            return Poll::Ready(None);
        }
        match state.poll_blob(cx) {
            Poll::Ready(Some(Err(err))) => {
                state.fail(err.to_string());
                Poll::Ready(Some(Err(crate::Error::new(err))))
            }
            value => value.map(|value| value.map(|value| value.map_err(crate::Error::new))),
        }
    }
}

#[cfg(test)]
mod tests;
