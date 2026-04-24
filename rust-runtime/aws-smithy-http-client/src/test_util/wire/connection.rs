/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Test harness for connection-level behavior testing.
//!
//! Simulates multiple IPs via TCP listeners on different loopback addresses
//! (`127.0.0.1`, `127.0.0.2`, etc.) sharing the same port.

use aws_smithy_runtime_api::client::dns::{DnsFuture, ResolveDns};
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Programmable behavior for a test endpoint per accepted connection.
#[derive(Debug, Clone)]
pub enum ConnectionBehavior {
    /// Accept TCP, send HTTP/1.1 response, keep connection open (reusable).
    Respond {
        /// HTTP status code to return.
        status: u16,
        /// Response body bytes.
        body: &'static [u8],
    },
    /// Accept TCP, immediately reset the connection (RST).
    ResetOnConnect,
    /// Accept TCP, hold open for duration, then close.
    HoldThenClose(std::time::Duration),
}

/// Recorded event from the test harness.
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// TCP connection accepted at an endpoint.
    TcpAccepted {
        /// The address of the endpoint that accepted the connection.
        endpoint_addr: SocketAddr,
    },
    /// DNS lookup performed.
    DnsLookup {
        /// The hostname that was looked up.
        hostname: String,
    },
}

/// A TCP endpoint bound to a specific address that executes programmed behaviors.
pub struct TestEndpoint {
    addr: SocketAddr,
    _task: JoinHandle<()>,
}

impl TestEndpoint {
    async fn bind(
        addr: &str,
        behaviors: Vec<ConnectionBehavior>,
        events: Arc<Mutex<Vec<ConnectionEvent>>>,
    ) -> Self {
        let listener = TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| panic!("failed to bind TCP listener to {addr}: {e}"));
        let addr = listener
            .local_addr()
            .expect("failed to get local address from listener");
        let behaviors = Arc::new(Mutex::new(VecDeque::from(behaviors)));

        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(_) => break,
                };
                events
                    .lock()
                    .expect("event lock poisoned")
                    .push(ConnectionEvent::TcpAccepted {
                        endpoint_addr: addr,
                    });
                let behavior = behaviors
                    .lock()
                    .expect("behavior lock poisoned")
                    .pop_front();
                tokio::spawn(handle_connection(stream, behavior, behaviors.clone()));
            }
        });

        Self { addr, _task: task }
    }

    /// The port this endpoint is listening on.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The IP address this endpoint is listening on.
    pub fn ip(&self) -> IpAddr {
        self.addr.ip()
    }

    /// The full socket address this endpoint is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    first_behavior: Option<ConnectionBehavior>,
    remaining: Arc<Mutex<VecDeque<ConnectionBehavior>>>,
) {
    let Some(behavior) = first_behavior else {
        drop(stream);
        return;
    };

    match behavior {
        ConnectionBehavior::ResetOnConnect => {
            // Set SO_LINGER to 0 to send TCP RST on close
            let sock = socket2::SockRef::from(&stream);
            sock.set_linger(Some(std::time::Duration::ZERO))
                .expect("failed to set SO_LINGER");
            drop(stream);
        }
        ConnectionBehavior::HoldThenClose(duration) => {
            tokio::time::sleep(duration).await;
            drop(stream);
        }
        ConnectionBehavior::Respond { status, body } => {
            if write_response(&mut stream, status, body).await.is_err() {
                return;
            }
            // Keep-alive loop: read next request, send next behavior's response
            loop {
                if read_request(&mut stream).await.is_err() {
                    return;
                }
                let next = remaining
                    .lock()
                    .expect("behavior lock poisoned")
                    .pop_front();
                match next {
                    Some(ConnectionBehavior::Respond { status, body }) => {
                        if write_response(&mut stream, status, body).await.is_err() {
                            return;
                        }
                    }
                    _ => return, // No more behaviors or non-Respond behavior: close
                }
            }
        }
    }
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let header = format!(
        "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<(), std::io::Error> {
    // Read until we see \r\n\r\n (end of HTTP headers)
    let mut buf = vec![0u8; 4096];
    let mut total = 0;
    loop {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "client closed connection",
            ));
        }
        total += n;
        if total >= 4 && buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
            return Ok(());
        }
    }
}

/// Mock DNS resolver that returns configured IPs and logs lookups.
#[derive(Debug, Clone)]
pub struct MockDnsResolver {
    responses: HashMap<String, Vec<IpAddr>>,
    events: Arc<Mutex<Vec<ConnectionEvent>>>,
}

impl MockDnsResolver {
    fn new(events: Arc<Mutex<Vec<ConnectionEvent>>>) -> Self {
        Self {
            responses: HashMap::new(),
            events,
        }
    }

    /// Add a DNS entry mapping hostname to IPs.
    fn with(mut self, hostname: &str, ips: Vec<IpAddr>) -> Self {
        self.responses.insert(hostname.to_string(), ips);
        self
    }
}

impl ResolveDns for MockDnsResolver {
    fn resolve_dns<'a>(&'a self, name: &'a str) -> DnsFuture<'a> {
        let ips = self.responses.get(name).cloned().unwrap_or_default();
        self.events
            .lock()
            .expect("event lock poisoned")
            .push(ConnectionEvent::DnsLookup {
                hostname: name.to_string(),
            });
        DnsFuture::ready(Ok(ips))
    }
}

/// Test harness for connection-level behavior testing.
pub struct ConnectionTestHarness {
    /// The test endpoints managed by this harness.
    pub endpoints: Vec<TestEndpoint>,
    events: Arc<Mutex<Vec<ConnectionEvent>>>,
    dns_resolver: MockDnsResolver,
}

impl ConnectionTestHarness {
    /// Create a new builder for the test harness.
    pub fn builder() -> HarnessBuilder {
        HarnessBuilder {
            endpoint_configs: Vec::new(),
            dns_entries: Vec::new(),
        }
    }

    /// Clone all recorded events.
    pub fn events(&self) -> Vec<ConnectionEvent> {
        self.events.lock().expect("event lock poisoned").clone()
    }

    /// Count of TCP accepted events.
    pub fn tcp_accepted_count(&self) -> usize {
        self.events()
            .iter()
            .filter(|e| matches!(e, ConnectionEvent::TcpAccepted { .. }))
            .count()
    }

    /// Count of TCP accepted events for a specific IP.
    pub fn tcp_accepted_by(&self, ip: IpAddr) -> usize {
        self.events()
            .iter()
            .filter(|e| matches!(e, ConnectionEvent::TcpAccepted { endpoint_addr } if endpoint_addr.ip() == ip))
            .count()
    }

    /// Count of DNS lookup events.
    pub fn dns_lookup_count(&self) -> usize {
        self.events()
            .iter()
            .filter(|e| matches!(e, ConnectionEvent::DnsLookup { .. }))
            .count()
    }

    /// Get a clone of the mock DNS resolver.
    pub fn dns_resolver(&self) -> MockDnsResolver {
        self.dns_resolver.clone()
    }
}

/// Builder for [`ConnectionTestHarness`].
pub struct HarnessBuilder {
    endpoint_configs: Vec<(IpAddr, Vec<ConnectionBehavior>)>,
    dns_entries: Vec<(String, Vec<IpAddr>)>,
}

impl HarnessBuilder {
    /// Add an endpoint with the given IP and behaviors.
    pub fn endpoint(mut self, ip: IpAddr, behaviors: Vec<ConnectionBehavior>) -> Self {
        self.endpoint_configs.push((ip, behaviors));
        self
    }

    /// Add a DNS entry mapping hostname to IPs.
    pub fn dns(mut self, hostname: &str, ips: Vec<IpAddr>) -> Self {
        self.dns_entries.push((hostname.to_string(), ips));
        self
    }

    /// Add a DNS entry mapping hostname to all configured endpoint IPs.
    pub fn dns_all(mut self, hostname: &str) -> Self {
        let ips: Vec<IpAddr> = self.endpoint_configs.iter().map(|(ip, _)| *ip).collect();
        self.dns_entries.push((hostname.to_string(), ips));
        self
    }

    /// Build the test harness, binding all endpoints.
    pub async fn build(self) -> ConnectionTestHarness {
        let events: Arc<Mutex<Vec<ConnectionEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let mut endpoints = Vec::new();

        let mut port = 0u16;
        for (i, (ip, behaviors)) in self.endpoint_configs.into_iter().enumerate() {
            let bind_addr = if i == 0 {
                format!("{ip}:0")
            } else {
                format!("{ip}:{port}")
            };
            let ep = TestEndpoint::bind(&bind_addr, behaviors, events.clone()).await;
            if i == 0 {
                port = ep.port();
            }
            endpoints.push(ep);
        }

        let mut resolver = MockDnsResolver::new(events.clone());
        for (hostname, ips) in self.dns_entries {
            resolver = resolver.with(&hostname, ips);
        }

        ConnectionTestHarness {
            endpoints,
            events,
            dns_resolver: resolver,
        }
    }
}
