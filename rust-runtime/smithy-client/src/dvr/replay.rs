/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::dvr::{Action, ConnectionId, Direction, Event};
use bytes::Bytes;
use http::{Request, Version};
use http_body::Body;
use smithy_http::body::SdkBody;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// Replay traffic recorded by a [`RecordingConnection`](RecordingConnection)
#[derive(Clone, Debug)]
pub struct ReplayingConnection {
    events: Arc<Mutex<HashMap<ConnectionId, VecDeque<Event>>>>,
    num_events: Arc<AtomicUsize>,
    recorded_requests: Arc<Mutex<HashMap<ConnectionId, http::Request<Bytes>>>>,
}

impl ReplayingConnection {
    fn next_id(&self) -> ConnectionId {
        ConnectionId(self.num_events.fetch_add(1, Ordering::Relaxed))
    }

    /// Return all the recorded requests for further analysis
    pub fn take_requests(&self) -> HashMap<ConnectionId, http::Request<Bytes>> {
        self.recorded_requests.lock().unwrap().drain().collect()
    }

    /// Build a replay connection from a sequence of events
    pub fn new(events: Vec<Event>) -> Self {
        let mut event_map = HashMap::new();
        for event in events {
            let event_buffer = event_map
                .entry(event.connection_id)
                .or_insert(VecDeque::new());
            event_buffer.push_back(event);
        }
        ReplayingConnection {
            events: Arc::new(Mutex::new(event_map)),
            num_events: Arc::new(AtomicUsize::new(0)),
            recorded_requests: Default::default(),
        }
    }
}

async fn replay_body(events: VecDeque<Event>, mut sender: hyper::body::Sender) {
    for event in events {
        match event.action {
            Action::Request { .. } => panic!(),
            Action::Response { .. } => panic!(),
            Action::Data {
                data,
                direction: Direction::Response,
            } => {
                sender
                    .send_data(Bytes::from(data.into_bytes()))
                    .await
                    .expect("this is in memory traffic that should not fail to send");
            }
            Action::Data {
                data: _data,
                direction: Direction::Request,
            } => {}
            Action::Eof {
                direction: Direction::Request,
                ..
            } => {}
            Action::Eof {
                direction: Direction::Response,
                ok: true,
                ..
            } => {
                drop(sender);
                break;
            }
            Action::Eof {
                direction: Direction::Response,
                ok: false,
                ..
            } => {
                sender.abort();
                break;
            }
        }
    }
}

fn convert_version(version: &str) -> Version {
    match version {
        "HTTP/1.1" => Version::HTTP_11,
        "HTTP/2.0" => Version::HTTP_2,
        _ => panic!("unsupported: {}", version),
    }
}

impl tower::Service<http::Request<SdkBody>> for ReplayingConnection {
    type Response = http::Response<SdkBody>;
    type Error = Box<dyn Error + Send + Sync + 'static>;

    #[allow(clippy::type_complexity)]
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: Request<SdkBody>) -> Self::Future {
        let event_id = self.next_id();
        let mut events = self
            .events
            .lock()
            .unwrap()
            .remove(&event_id)
            .expect("no data for event id");
        let _initial_request = events.pop_front().unwrap();
        let (sender, response_body) = hyper::Body::channel();
        let body = SdkBody::from(response_body);
        let recording = self.recorded_requests.clone();
        let mut request_complete = Some(tokio::spawn(async move {
            let mut data_read = vec![];
            while let Some(data) = req.body_mut().data().await {
                data_read
                    .extend_from_slice(data.expect("in memory request should not fail").as_ref())
            }
            recording
                .lock()
                .unwrap()
                .insert(event_id, req.map(|_| Bytes::from(data_read)));
        }));
        let fut = async move {
            let resp = loop {
                let event = events
                    .pop_front()
                    .expect("no events, needed a response event");
                match event.action {
                    // to ensure deterministic behavior if the request EOF happens first in the log,
                    // wait for the request body to be done before returning a response.
                    Action::Eof {
                        direction: Direction::Request,
                        ..
                    } => match request_complete.take() {
                        Some(handle) => {
                            let _ = handle.await;
                        }
                        None => panic!("double await on request eof"),
                    },
                    Action::Request { .. } => panic!("invalid"),
                    Action::Response {
                        response: Err(error),
                    } => break Err(error.0.into()),
                    Action::Response {
                        response: Ok(response),
                    } => {
                        let mut builder = http::Response::builder()
                            .status(response.status)
                            .version(convert_version(&response.version));
                        for (name, values) in response.headers {
                            for value in values {
                                builder = builder.header(&name, &value);
                            }
                        }
                        tokio::spawn(async move {
                            replay_body(events, sender).await;
                            // insert the finalized body into
                        });
                        break Ok(builder.body(body).expect("valid builder"));
                    }

                    Action::Data {
                        direction: Direction::Request,
                        data: _data,
                    } => {
                        tracing::info!("get request data");
                    }
                    Action::Eof {
                        direction: Direction::Response,
                        ..
                    } => panic!("got eof before response"),

                    Action::Data {
                        data: _,
                        direction: Direction::Response,
                    } => panic!("got response data before response"),
                }
            };
            resp
        };
        Box::pin(fut)
    }
}
