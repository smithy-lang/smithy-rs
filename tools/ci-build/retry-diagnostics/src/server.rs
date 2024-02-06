/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::{Display, Formatter};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::scenario::{Scenario, ScenarioResponse};
use bytes::Bytes;
use http::Uri;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::{service_fn, HttpService};
use hyper::{HeaderMap, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot::{channel, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task;
use tracing::{debug, info};

enum Event {
    Reconnect,
    Request { body: Bytes, headers: HeaderMap },
}

#[derive(Debug)]
enum IncomingEvent {
    NewConnection,
    Request(Request<Bytes>),
}
type Chan = IncomingEvent;
#[derive(Clone)]
struct DiagnosticServer {
    inner: Arc<Mutex<Log>>,
}

impl DiagnosticServer {
    fn new(scenarios: Vec<Scenario>) -> (Self, Receiver<Report>) {
        let (tx, rx) = channel();
        (
            Self {
                inner: Arc::new(Mutex::new(Log::new(tx, scenarios))),
            },
            rx,
        )
    }

    pub async fn handle(&self, req: Chan) -> Response<Bytes> {
        let mut inner = self.inner.lock().await;
        inner.handle(req).await
    }
}

enum Action {
    Done,
    Resp(Response<Bytes>),
}

#[derive(Debug)]
struct TestRun {
    response: Scenario,
    request: FirstRequest,
    num_retries: u32,
    num_reconnects: u32,
}

impl TestRun {
    fn request_applies(&self, req: &IncomingEvent) -> bool {
        match (&self.request, req) {
            (FirstRequest::Connecting, _) => true,
            (FirstRequest::Request { body, uri }, IncomingEvent::Request(req)) => {
                let matches = req.body() == body && req.uri() == uri;
                if !matches {
                    debug!("next request: {:?} vs. {:?}", req.body(), body);
                }
                matches
            }
            (_, IncomingEvent::NewConnection) => true,
        }
    }
}

#[derive(Debug)]
enum FirstRequest {
    Connecting,
    Request { body: Bytes, uri: Uri },
}

struct Log {
    scenarios_to_run: Vec<Scenario>,
    shutdown: Option<Sender<Report>>,
    finished: Vec<TestRun>,
    active: Option<TestRun>,
}

#[derive(Debug)]
pub struct Report {
    runs: Vec<TestRun>,
}

impl Display for Report {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let longest_name = self
            .runs
            .iter()
            .map(|r| r.response.name.len() + 10)
            .max()
            .unwrap_or(0);
        for run in &self.runs {
            writeln!(
                f,
                "status_code={} {:width$} attempts={} reconnects={}",
                &run.response.response.status_code(),
                &run.response.name,
                run.num_retries,
                run.num_reconnects,
                width = longest_name
            )?
        }
        Ok(())
    }
}

impl Log {
    pub fn new(sender: Sender<Report>, mut scenarios_to_run: Vec<Scenario>) -> Self {
        scenarios_to_run.reverse();
        Self {
            scenarios_to_run,
            shutdown: Some(sender),
            finished: vec![],
            active: None,
        }
    }

    fn active_scenario(&mut self) -> Option<&mut TestRun> {
        self.active.as_mut()
    }

    fn needs_new_scenario(&self, ev: &Chan) -> bool {
        match &self.active {
            Some(scenario) => {
                let applies = scenario.request_applies(ev);
                if !applies {
                    debug!("request does not apply to {:?}", scenario);
                }
                !applies
            }
            None => {
                debug!("no scenario available, needs advance");
                true
            }
        }
    }

    fn advance_scenario(&mut self, ev: &Chan) {
        if let Some(active) = self.active.take() {
            self.finished.push(active);
        }
        let Some(next_scenario) = self.scenarios_to_run.pop() else {
            debug!("no more scenarios available to run {:?}", ev);
            self.active = None;
            return;
        };
        self.active = Some(TestRun {
            response: next_scenario,
            num_retries: 0,
            num_reconnects: 0,
            request: match ev {
                Chan::NewConnection => FirstRequest::Connecting,
                Chan::Request(req) => FirstRequest::Request {
                    body: req.body().clone(),
                    uri: req.uri().clone(),
                },
            },
        });
    }

    pub async fn handle(&mut self, req: Chan) -> Response<Bytes> {
        if self.needs_new_scenario(&req) {
            debug!("advancing to next scenario on {:?}", req);
            self.advance_scenario(&req);
        } else {
            debug!("request applies to current scenario");
        }
        if let Some(scenario) = self.active_scenario() {
            match req {
                IncomingEvent::NewConnection => {
                    debug!("reconnect");
                    scenario.num_reconnects += 1
                }
                IncomingEvent::Request(req) => {
                    debug!("retry");
                    if matches!(scenario.request, FirstRequest::Connecting) {
                        scenario.request = FirstRequest::Request {
                            body: req.body().clone(),
                            uri: req.uri().clone(),
                        };
                    }
                    scenario.num_retries += 1
                }
            }
            let mut resp = Response::builder();
            match &scenario.response.response {
                ScenarioResponse::Timeout => panic!("unsupported"),
                ScenarioResponse::Response {
                    status_code,
                    body,
                    headers,
                } => {
                    for (k, v) in headers {
                        resp = resp.header(k, v)
                    }
                    resp.status(*status_code)
                        .body(Bytes::from(body.clone()))
                        .unwrap()
                }
            }
        } else {
            if let Some(shutdown) = self.shutdown.take() {
                shutdown
                    .send(Report {
                        runs: std::mem::take(&mut self.finished),
                    })
                    .unwrap();
            }
            Response::builder().body(Bytes::new()).unwrap()
        }
    }
}
pub(crate) async fn start_server(scenarios: Vec<Scenario>) -> anyhow::Result<Report> {
    let (tx, cancellation) = DiagnosticServer::new(scenarios);

    // We start a loop to continuously accept incoming connections
    task::spawn(main_loop(tx));
    Ok(cancellation.await?)
}

async fn main_loop(tx: DiagnosticServer) -> anyhow::Result<()> {
    loop {
        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

        // We create a TcpListener and bind it to 127.0.0.1:3000
        let listener = TcpListener::bind(addr).await?;
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let tx = tx.clone();
        tx.handle(IncomingEvent::NewConnection).await;

        if let Err(err) = http1::Builder::new()
            .serve_connection(
                io,
                service_fn(move |req| {
                    let tx = tx.clone();
                    async move {
                        let mut req: Request<hyper::body::Incoming> = req;
                        let data = req.body_mut().collect().await?.to_bytes();
                        let req = req.map(|_b| data);
                        reply(tx.handle(IncomingEvent::Request(req)).await)
                    }
                }),
            )
            .await
        {
            println!("Error serving connection: {:?}", err);
        }
    }
}

fn reply(resp: http::Response<Bytes>) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    Ok(resp.map(Full::new))
}
