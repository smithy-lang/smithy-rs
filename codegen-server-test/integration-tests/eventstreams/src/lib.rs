/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Manual event stream client for testing purposes.

use aws_smithy_eventstream::frame::{
    write_message_to, DecodedFrame, MessageFrameDecoder, SignMessage,
};
use aws_smithy_types::event_stream::Message;
use http_body_util::{BodyExt, StreamBody};
use hyper::{
    body::{Bytes, Frame},
    Request, Uri,
};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// A manual event stream client that connects to a real service.
pub struct ManualEventStreamClient {
    message_sender: mpsc::Sender<Message>,
    response_receiver: mpsc::Receiver<Result<Message, String>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl ManualEventStreamClient {
    /// Creates a client that connects to a real H2 service on localhost.
    pub async fn connect_to_service(
        addr: SocketAddr,
        path: &str,
        headers: Vec<(&str, &str)>,
    ) -> Result<Self, String> {
        let (message_sender, mut message_receiver) = mpsc::channel(100);
        let (response_sender, response_receiver) = mpsc::channel(100);
        let (frame_sender, frame_receiver) = mpsc::channel(100);

        let client = Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build_http::<StreamBody<ReceiverStream<Result<Frame<Bytes>, std::io::Error>>>>();

        let uri = format!("http://{addr}{path}");

        // Task to convert messages to frames
        tokio::spawn(async move {
            while let Some(message) = message_receiver.recv().await {
                let mut buffer = Vec::new();
                if let Err(_) = write_message_to(&message, &mut buffer) {
                    break;
                }
                let _ = frame_sender
                    .send(Ok(Frame::data(Bytes::from(buffer))))
                    .await;
            }
        });
        let headers = headers
            .iter()
            .map(|s| (s.0.to_string(), s.1.to_string()))
            .collect::<Vec<_>>();

        let handle = tokio::spawn(async move {
            let stream = ReceiverStream::new(frame_receiver);
            let body = StreamBody::new(stream);

            let uri: Uri = dbg!(uri).parse().expect("invalid URI");
            let mut req = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/vnd.amazon.eventstream");

            for (key, value) in &headers {
                req = req.header(key.to_string(), value.to_string());
            }

            let request = match req.body(body) {
                Ok(req) => req,
                Err(e) => {
                    let _ = response_sender
                        .send(Err(format!("Failed to build request: {e}")))
                        .await;
                    return;
                }
            };

            match client.request(request).await {
                Ok(response) => {
                    let mut body = response.into_body();
                    let mut decoder = MessageFrameDecoder::new();

                    while let Some(frame_result) = body.frame().await {
                        match frame_result {
                            Ok(frame) => {
                                if let Some(data) = frame.data_ref() {
                                    let mut data_slice = data.as_ref();
                                    match decoder.decode_frame(&mut data_slice) {
                                        Ok(DecodedFrame::Complete(msg)) => {
                                            let _ = response_sender.send(Ok(msg)).await;
                                        }
                                        Ok(DecodedFrame::Incomplete) => continue,
                                        Err(e) => {
                                            let _ = response_sender
                                                .send(Err(format!("Decode error: {}", e)))
                                                .await;
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = response_sender
                                    .send(Err(format!("Frame error: {}", e)))
                                    .await;
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = response_sender
                        .send(Err(format!("Request failed: {}", e)))
                        .await;
                }
            }
        });

        Ok(Self {
            message_sender,
            response_receiver,
            _handle: handle,
        })
    }

    /// Sends a message.
    pub async fn send(&mut self, message: Message) -> Result<(), String> {
        self.message_sender
            .send(message)
            .await
            .map_err(|e| format!("Send failed: {}", e))
    }

    /// Receives the next response message.
    pub async fn recv(&mut self) -> Option<Result<Message, String>> {
        self.response_receiver.recv().await
    }
}

/// Builder for creating event stream messages with signing support.
pub struct MessageBuilder {
    signer: Option<Box<dyn SignMessage + Send>>,
}

impl MessageBuilder {
    pub fn new() -> Self {
        Self { signer: None }
    }

    pub fn with_signer(mut self, signer: Box<dyn SignMessage + Send>) -> Self {
        self.signer = Some(signer);
        self
    }

    pub fn build_message(
        &mut self,
        message: Message,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        match &mut self.signer {
            Some(signer) => signer.sign(message),
            None => Ok(message),
        }
    }
}

impl Default for MessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_types::event_stream::Message;

    #[tokio::test]
    async fn test_message_builder() {
        let mut builder = MessageBuilder::new();
        let message = Message::new(&b"test"[..]);

        let result = builder.build_message(message);
        assert!(result.is_ok());
    }
}
