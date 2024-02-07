/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::{Display, Formatter};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;

use aws_smithy_async::future::never::Never;
use bytes::Bytes;
use http::Uri;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot::{channel, Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task;
use tracing::debug;

use crate::scenario::{Scenario, ScenarioResponse};

pub(crate) trait Progress: Send + Sync + 'static {
    fn scenarios_remaining(&self, num_remaining: usize);
}

type SharedProgress = Arc<dyn Progress>;

#[derive(Debug)]
enum IncomingEvent<'a> {
    NewConnection,
    Request(&'a Request<Bytes>),
}
type Chan<'a> = IncomingEvent<'a>;
#[derive(Clone)]
struct DiagnosticServer {
    inner: Arc<Mutex<Log>>,
}

impl DiagnosticServer {
    fn new(scenarios: Vec<Scenario>, progress: SharedProgress) -> (Self, Receiver<Report>) {
        let (cancellation_tx, cancellation_rx) = channel();
        (
            Self {
                inner: Arc::new(Mutex::new(Log::new(cancellation_tx, scenarios, progress))),
            },
            cancellation_rx,
        )
    }

    pub async fn new_connection(&self) {
        let mut inner = self.inner.lock().await;
        inner.new_connection().await;
    }

    pub async fn handle(&self, req: Request<Bytes>) -> Response<Bytes> {
        let mut inner = self.inner.lock().await;
        match inner.handle(req).await {
            LogResponse::Response(response) => response,
            LogResponse::Timeout => {
                drop(inner);
                Never::new().await;
                unreachable!()
            }
        }
    }
}

#[derive(Debug)]
struct TestRun {
    response: Scenario,
    request: FirstRequest,
    num_retries: u32,
    num_reconnects: u32,
    start_time: SystemTime,
    end_time: Option<SystemTime>,
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
    progress: SharedProgress,
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
            let total_time = run
                .end_time
                .map(|et| et.duration_since(run.start_time).unwrap())
                .map(|dur| format!("{:.2}s", dur.as_secs_f64()))
                .unwrap_or("N/A".to_string());
            writeln!(
                f,
                "status_code={:<3} {:width$} attempts={} reconnects={} total_time={}",
                &run.response.response.status_code(),
                &run.response.name,
                run.num_retries,
                run.num_reconnects,
                total_time,
                width = longest_name
            )?
        }
        Ok(())
    }
}

enum LogResponse {
    Timeout,
    Response(Response<Bytes>),
}

impl Log {
    pub fn new(
        sender: Sender<Report>,
        mut scenarios_to_run: Vec<Scenario>,
        progress: SharedProgress,
    ) -> Self {
        scenarios_to_run.reverse();
        Self {
            scenarios_to_run,
            shutdown: Some(sender),
            finished: vec![],
            active: None,
            progress,
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
        if let Some(mut active) = self.active.take() {
            active.end_time = Some(SystemTime::now());
            self.finished.push(active);
        }
        self.progress
            .scenarios_remaining(self.scenarios_to_run.len());
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
            start_time: SystemTime::now(),
            end_time: None,
        });
    }

    pub async fn new_connection(&mut self) {
        if self.active_scenario().is_none() {
            self.advance_scenario(&Chan::NewConnection);
        } else {
            debug!("new connection does not need a new scenario");
        }
        if let Some(scenario) = self.active_scenario() {
            scenario.num_reconnects += 1
        }
    }

    pub async fn handle(&mut self, req: Request<Bytes>) -> LogResponse {
        let ev = IncomingEvent::Request(&req);
        if self.needs_new_scenario(&ev) {
            debug!("advancing to next scenario on {:?}", req);
            self.advance_scenario(&ev);
        } else {
            debug!("request applies to current scenario");
        }
        if let Some(scenario) = self.active_scenario() {
            if matches!(scenario.request, FirstRequest::Connecting) {
                scenario.request = FirstRequest::Request {
                    body: req.body().clone(),
                    uri: req.uri().clone(),
                };
            }
            scenario.num_retries += 1;
            match &scenario.response.response {
                ScenarioResponse::Timeout => LogResponse::Timeout,
                ScenarioResponse::Response {
                    status_code,
                    body,
                    headers,
                } => {
                    let mut resp = Response::builder();
                    for (k, v) in headers {
                        resp = resp.header(k, v)
                    }
                    LogResponse::Response(
                        resp.status(*status_code)
                            .body(Bytes::from(body.clone()))
                            .unwrap(),
                    )
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
            LogResponse::Response(Response::builder().body(Bytes::new()).unwrap())
        }
    }
}
pub(crate) async fn start_server(
    scenarios: Vec<Scenario>,
    progress: SharedProgress,
) -> anyhow::Result<Report> {
    let (tx, cancellation) = DiagnosticServer::new(scenarios, progress);

    // We start a loop to continuously accept incoming connections
    task::spawn(main_loop(tx));
    Ok(cancellation.await?)
}

async fn main_loop(tx: DiagnosticServer) -> anyhow::Result<()> {
    // Use an adapter to access something implementing `tokio::io` traits as if they implement
    // `hyper::rt` IO traits.
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // We create a TcpListener and bind it to 127.0.0.1:3000
    let listener = TcpListener::bind(addr).await?;
    loop {
        debug!("waiting for new connections");
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let tx = tx.clone();
        tx.new_connection().await;

        if let Err(err) = http1::Builder::new()
            .serve_connection(
                io,
                service_fn(move |req| {
                    debug!("request received");
                    let tx = tx.clone();
                    async move {
                        let mut req: Request<hyper::body::Incoming> = req;
                        let data = req.body_mut().collect().await?.to_bytes();
                        let req = req.map(|_b| data);
                        reply(tx.handle(req).await)
                    }
                }),
            )
            .await
        {
            debug!("Error serving connection: {:?}", err);
        }
    }
}

fn reply(resp: http::Response<Bytes>) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    Ok(resp.map(Full::new))
}
