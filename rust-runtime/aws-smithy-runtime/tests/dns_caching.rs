/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(all(feature = "client", feature = "rt-tokio",))]

use aws_smithy_runtime::client::dns::CachingDnsResolver;
use aws_smithy_runtime_api::client::dns::ResolveDns;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    time::Instant,
};
use tokio::test;

// Note: I couldn't come up with a way to mock the DNS returns to hickory so these
// tests actually hit the network. We should ideally find a better way to do this.

#[test]
async fn test_dns_caching_performance() {
    let resolver = CachingDnsResolver::default();
    let hostname = "example.com";

    // First resolution should hit the network
    let start = Instant::now();
    let first_result = resolver.resolve_dns(hostname).await;
    let first_duration = start.elapsed();

    let first_ips = first_result.unwrap();
    assert!(!first_ips.is_empty());

    // Second resolution should hit the cache
    let start = Instant::now();
    let second_result = resolver.resolve_dns(hostname).await;
    let second_duration = start.elapsed();

    let second_ips = second_result.unwrap();

    // Verify same IPs returned
    assert_eq!(first_ips, second_ips);

    // Cache hit should be faster
    assert!(second_duration < first_duration);
}

#[test]
#[tracing_test::traced_test]
async fn test_dns_cache_size_limit() {
    let mut records = HashMap::new();
    records.insert(
        "example.com".to_string(),
        IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)),
    );
    records.insert("aws.com".to_string(), IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8)));

    let dns_server = test_dns_server::TestDnsServer::start(records)
        .await
        .unwrap();

    let (dns_ip, dns_port) = dns_server.addr();
    println!("DNS_IP: {dns_ip:#?}, DNS_PORT: {dns_port:#?}");
    let resolver = CachingDnsResolver::builder()
        .nameservers(&[dns_ip], dns_port)
        .cache_size(1)
        .build();

    // Resolve first hostname
    let result1 = resolver.resolve_dns("example.com").await;
    // assert!(result1.is_ok());

    // Resolve second hostname (should not be placed into cache because result1 is already occupying
    // the single allocated space and entries are only evicted from the cache when their TTL expires)
    let result2 = resolver.resolve_dns("aws.com").await;
    // assert!(result2.is_ok());

    println!("RESULT1: {result1:#?}");
    println!("RESULT2: {result2:#?}");

    let start = Instant::now();
    let result2_again = resolver.resolve_dns("aws.com").await;
    let result2_again_duration = start.elapsed();

    let start = Instant::now();
    let result1_again = resolver.resolve_dns("example.com").await;
    let result1_again_duration = start.elapsed();

    assert!(result1_again.is_ok());
    assert!(result2_again.is_ok());

    // result1_again should be resolved more quickly than result2_again
    println!("result1_again_duration: {:?}", result1_again_duration);
    println!("result2_again_duration: {:?}", result2_again_duration);
    assert!(result1_again_duration < result2_again_duration);
}

#[test]
async fn test_dns_error_handling() {
    let resolver = CachingDnsResolver::default();

    // Try to resolve an invalid hostname
    let result = resolver
        .resolve_dns("invalid.nonexistent.domain.test")
        .await;
    assert!(result.is_err());
}
// Test utility for creating a local DNS server
#[cfg(test)]
mod test_dns_server {
    use std::{
        collections::HashMap,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::{net::UdpSocket, sync::Notify, task::JoinHandle};

    pub struct TestDnsServer {
        handle: JoinHandle<()>,
        addr: SocketAddr,
        shutdown: Arc<Notify>,
    }

    impl TestDnsServer {
        pub async fn start(
            records: HashMap<String, IpAddr>,
        ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            // localhost, random port
            let socket = UdpSocket::bind("127.0.0.1:0").await?;
            let addr = socket.local_addr()?;

            let shutdown = Arc::new(Notify::new());
            let shutdown_clone = shutdown.clone();

            let handle = tokio::spawn(async move {
                let mut buf = [0; 512];
                loop {
                    tokio::select! {
                        _ = shutdown_clone.notified() => break,
                        result = socket.recv_from(&mut buf) => {
                            // println!("IN SOCKET RECV_FROM: {buf:#?}");
                            if let Ok((len, src)) = result {
                                // Simple DNS response - just echo back with a mock A record
                                // This is a minimal implementation for testing purposes
                                let response = create_dns_response(&buf[..len], &records);
                                // println!("SOCKET SEND RES: {response:#?}");
                                let res = socket.send_to(&response, src).await;

                            }
                        }
                    }
                }
            });

            Ok(TestDnsServer {
                handle,
                addr,
                shutdown,
            })
        }

        pub fn addr(&self) -> (IpAddr, u16) {
            (self.addr.ip(), self.addr.port())
        }
    }

    impl Drop for TestDnsServer {
        fn drop(&mut self) {
            self.shutdown.notify_one();
            self.handle.abort();
        }
    }

    fn create_dns_response(query: &[u8], records: &HashMap<String, IpAddr>) -> Vec<u8> {
        let parsed = DnsQuery::parse(query).unwrap_or_default();
        let ip = records
            .get(&parsed.domain)
            .copied()
            .unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));

        let response = DnsResponse {
            id: parsed.id,
            flags: 0x8180, // Standard response flags
            question: parsed.domain,
            answer_ip: ip,
            ttl: 300,
        };

        response.serialize()
    }

    #[derive(Debug, Default)]
    struct DnsQuery {
        id: u16,
        flags: u16,
        question_count: u16,
        domain: String,
        query_type: u16,
        query_class: u16,
    }

    impl DnsQuery {
        fn parse(data: &[u8]) -> Option<Self> {
            if data.len() < 12 {
                return None;
            }

            let id = u16::from_be_bytes([data[0], data[1]]);
            let flags = u16::from_be_bytes([data[2], data[3]]);
            let question_count = u16::from_be_bytes([data[4], data[5]]);

            if question_count == 0 {
                return None;
            }

            // Parse domain name starting at byte 12
            let mut pos = 12;
            let mut domain = String::new();

            while pos < data.len() {
                let len = data[pos] as usize;
                if len == 0 {
                    pos += 1;
                    break;
                }

                if !domain.is_empty() {
                    domain.push('.');
                }

                pos += 1;
                if pos + len > data.len() {
                    return None;
                }

                if let Ok(label) = std::str::from_utf8(&data[pos..pos + len]) {
                    domain.push_str(label);
                }
                pos += len;
            }

            if pos + 4 > data.len() {
                return None;
            }

            let query_type = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let query_class = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);

            Some(DnsQuery {
                id,
                flags,
                question_count,
                domain,
                query_type,
                query_class,
            })
        }
    }

    #[derive(Debug)]
    struct DnsResponse {
        id: u16,
        flags: u16,
        question: String,
        answer_ip: IpAddr,
        ttl: u32,
    }

    impl DnsResponse {
        fn serialize(&self) -> Vec<u8> {
            let mut response = Vec::new();

            // Header (12 bytes)
            response.extend_from_slice(&self.id.to_be_bytes());
            response.extend_from_slice(&self.flags.to_be_bytes());
            response.extend_from_slice(&1u16.to_be_bytes()); // Questions: 1
            response.extend_from_slice(&1u16.to_be_bytes()); // Answers: 1
            response.extend_from_slice(&0u16.to_be_bytes()); // Authority: 0
            response.extend_from_slice(&0u16.to_be_bytes()); // Additional: 0

            // Question section
            for label in self.question.split('.') {
                response.push(label.len() as u8);
                response.extend_from_slice(label.as_bytes());
            }
            response.push(0); // End of name
            response.extend_from_slice(&1u16.to_be_bytes()); // Type A
            response.extend_from_slice(&1u16.to_be_bytes()); // Class IN

            // Answer section
            response.extend_from_slice(&[0xc0, 0x0c]); // Name pointer
            response.extend_from_slice(&1u16.to_be_bytes()); // Type A
            response.extend_from_slice(&1u16.to_be_bytes()); // Class IN
            response.extend_from_slice(&self.ttl.to_be_bytes()); // TTL

            match self.answer_ip {
                IpAddr::V4(ipv4) => {
                    response.extend_from_slice(&4u16.to_be_bytes()); // Data length
                    response.extend_from_slice(&ipv4.octets());
                }
                IpAddr::V6(_) => {
                    response.extend_from_slice(&4u16.to_be_bytes()); // Fallback to IPv4
                    response.extend_from_slice(&[127, 0, 0, 1]);
                }
            }

            response
        }
    }
}
