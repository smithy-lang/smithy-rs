/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{
    Action, BodyData, ConnectionId, Direction, Error, Event, NetworkTraffic, Request, Response,
    Version,
};
use aws_smithy_runtime_api::client::connector_metadata::ConnectorMetadata;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::task::JoinHandle;
use {std::fs, std::io};

/// Recording client
///
/// `RecordingClient` wraps an inner connection and records all traffic, enabling traffic replay.
///
/// # Example
///
/// ```rust,ignore
/// use aws_smithy_http_client::test_util::dvr::RecordingClient;
/// use aws_sdk_s3::{Client, Config};
///
/// #[tokio::test]
/// async fn test_content_length_enforcement_is_not_applied_to_head_request() {
///     // Create a recording client that wraps a default HTTPS connector
///     let http_client = RecordingClient::https();
///
///     // Since we need to send a real request for this,
///     // you'll need to use your real credentials.
///     let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
///     let config = Config::from(&config).to_builder()
///         .http_client(http_client.clone())
///         .region(Region::new("us-east-1"))
///         .build();
///
///     let client = Client::from_conf(config);
///     let _resp = client
///         .head_object()
///         .key("some-test-file.txt")
///         .bucket("your-test-bucket")
///         .send()
///         .await
///         .unwrap();
///
///     // If the request you want to record has a body, don't forget to poll
///     // the body to completion BEFORE calling `dump_to_file`. Otherwise, your
///     // test json won't include the body.
///     // let _body = _resp.body.collect().await.unwrap();
///
///     // This path is relative to your project or workspace `Cargo.toml` file.
///     http_client.dump_to_file("tests/data/content-length-enforcement/head-object.json").unwrap();
/// }
/// ```
#[derive(Clone, Debug)]
pub struct RecordingClient {
    pub(crate) data: Arc<Mutex<Vec<Event>>>,
    pub(crate) num_events: Arc<AtomicUsize>,
    pub(crate) inner: SharedHttpConnector,
}

#[cfg(feature = "legacy-rustls-ring")]
impl RecordingClient {
    /// Construct a recording connection wrapping a default HTTPS implementation without any timeouts.
    pub fn https() -> Self {
        #[allow(deprecated)]
        use crate::hyper_014::HyperConnector;
        Self {
            data: Default::default(),
            num_events: Arc::new(AtomicUsize::new(0)),
            #[allow(deprecated)]
            inner: SharedHttpConnector::new(HyperConnector::builder().build_https()),
        }
    }
}

impl RecordingClient {
    /// Create a new recording connection from a connection
    pub fn new(underlying_connector: impl HttpConnector + 'static) -> Self {
        Self {
            data: Default::default(),
            num_events: Arc::new(AtomicUsize::new(0)),
            inner: underlying_connector.into_shared(),
        }
    }

    /// Return the traffic recorded by this connection
    pub fn events(&self) -> MutexGuard<'_, Vec<Event>> {
        self.data.lock().unwrap()
    }

    /// NetworkTraffic struct suitable for serialization
    pub fn network_traffic(&self) -> NetworkTraffic {
        NetworkTraffic {
            events: self.events().clone(),
            docs: Some("todo docs".into()),
            version: Version::V0,
        }
    }

    /// Dump the network traffic to a file
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        fs::write(
            path,
            serde_json::to_string(&self.network_traffic()).unwrap(),
        )
    }

    fn next_id(&self) -> ConnectionId {
        ConnectionId(self.num_events.fetch_add(1, Ordering::Relaxed))
    }
}

/// Record a request body, preferring a buffered approach for non-streaming bodies.
///
/// For request bodies that have their content available as bytes, this buffers
/// the body in memory to avoid timing issues with channel-based streaming.
/// Streaming request bodies fall back to the channel-based recorder.
fn record_request_body(
    body: &mut SdkBody,
    event_id: ConnectionId,
    direction: Direction,
    event_bus: Arc<Mutex<Vec<Event>>>,
) {
    if let Some(bytes) = body.bytes() {
        let data = Bytes::copy_from_slice(bytes);
        let mut events = event_bus.lock().unwrap();
        events.push(Event {
            connection_id: event_id,
            action: Action::Data {
                data: BodyData::from(data.clone()),
                direction,
            },
        });
        events.push(Event {
            connection_id: event_id,
            action: Action::Eof {
                ok: true,
                direction,
            },
        });
        *body = SdkBody::from(data);
    } else {
        record_body(body, event_id, direction, event_bus);
    }
}

/// Record body data using a channel-based streaming approach.
///
/// This spawns a task that reads from the original body and forwards data
/// through a channel to a replacement body. This is used for response bodies
/// and streaming request bodies.
fn record_body(
    body: &mut SdkBody,
    event_id: ConnectionId,
    direction: Direction,
    event_bus: Arc<Mutex<Vec<Event>>>,
) -> JoinHandle<()> {
    // Capture the content length so the replacement body reports the correct size_hint()
    let content_length = body.content_length();
    let (sender, output_body) = crate::test_util::body::channel_body_with_size_hint(content_length);
    let real_body = std::mem::replace(body, output_body);

    tokio::spawn(async move {
        let mut real_body = real_body;
        let mut sender = sender;
        loop {
            let data = crate::test_util::body::next_data_frame(&mut real_body).await;
            match data {
                Some(Ok(data)) => {
                    event_bus.lock().unwrap().push(Event {
                        connection_id: event_id,
                        action: Action::Data {
                            data: BodyData::from(data.clone()),
                            direction,
                        },
                    });
                    // Forward the data to the replacement body
                    if sender.send_data(data).await.is_err() {
                        event_bus.lock().unwrap().push(Event {
                            connection_id: event_id,
                            action: Action::Eof {
                                direction: direction.opposite(),
                                ok: false,
                            },
                        });
                        break;
                    }
                }
                None => {
                    event_bus.lock().unwrap().push(Event {
                        connection_id: event_id,
                        action: Action::Eof {
                            ok: true,
                            direction,
                        },
                    });
                    drop(sender);
                    break;
                }
                Some(Err(_err)) => {
                    event_bus.lock().unwrap().push(Event {
                        connection_id: event_id,
                        action: Action::Eof {
                            ok: false,
                            direction,
                        },
                    });
                    sender.abort();
                    break;
                }
            }
        }
    })
}

impl HttpConnector for RecordingClient {
    fn call(&self, mut request: HttpRequest) -> HttpConnectorFuture {
        let event_id = self.next_id();

        // A request has three phases:
        // 1. A "Request" phase: initial HTTP request (headers, method, URI)
        // 2. A body phase: may contain multiple data segments
        // 3. A finalization phase: EOF is sent to indicate the body is complete

        // Phase 1: Record the initial request
        self.data.lock().unwrap().push(Event {
            connection_id: event_id,
            action: Action::Request {
                request: Request::from(&request),
            },
        });

        // Phase 2 & 3: Record the request body
        // Use buffered recording for request bodies to avoid race conditions
        // with channel-based streaming that can cause HTTP/2 framing issues.
        record_request_body(
            request.body_mut(),
            event_id,
            Direction::Request,
            self.data.clone(),
        );

        let events = self.data.clone();
        let resp_fut = self.inner.call(request);

        let fut = async move {
            let resp = resp_fut.await;
            match resp {
                Ok(mut resp) => {
                    // Record the initial response
                    events.lock().unwrap().push(Event {
                        connection_id: event_id,
                        action: Action::Response {
                            response: Ok(Response::from(&resp)),
                        },
                    });

                    // Record the response body using channel-based streaming
                    // (this works fine for responses since we're consuming, not producing)
                    record_body(resp.body_mut(), event_id, Direction::Response, events);
                    Ok(resp)
                }
                Err(e) => {
                    events.lock().unwrap().push(Event {
                        connection_id: event_id,
                        action: Action::Response {
                            response: Err(Error(format!("{}", &e))),
                        },
                    });
                    Err(e)
                }
            }
        };
        HttpConnectorFuture::new(fut)
    }
}

impl HttpClient for RecordingClient {
    fn http_connector(
        &self,
        _: &HttpConnectorSettings,
        _: &RuntimeComponents,
    ) -> SharedHttpConnector {
        self.clone().into_shared()
    }

    fn connector_metadata(&self) -> Option<ConnectorMetadata> {
        Some(ConnectorMetadata::new("recording-client", None))
    }
}
