/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use tokio_rustls::{
    rustls::{Certificate, PrivateKey, ServerConfig},
    TlsAcceptor,
};

// Returns a `TlsAcceptor` that can be used to create `TlsListener`
// which then can be used with Hyper.
pub fn acceptor(cert_path: &str, key_path: &str) -> TlsAcceptor {
    let certs = load_certs(cert_path);
    let key = load_key(key_path);
    let mut server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("could not create server config");

    // If we don't state we are accepting "h2", clients by default don't negotiate way up to http2.
    server_config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

    TlsAcceptor::from(Arc::new(server_config))
}

fn load_certs(path: &str) -> Vec<Certificate> {
    let mut reader = BufReader::new(File::open(path).expect("could not open certificate"));
    rustls_pemfile::certs(&mut reader)
        .expect("could not parse certificate")
        .into_iter()
        .map(Certificate)
        .collect()
}

fn load_key(path: &str) -> PrivateKey {
    let mut reader = BufReader::new(File::open(path).expect("could not open private key"));
    loop {
        match rustls_pemfile::read_one(&mut reader).expect("could not parse private key") {
            Some(rustls_pemfile::Item::RSAKey(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::PKCS8Key(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::ECKey(key)) => return PrivateKey(key),
            None => break,
            _ => {}
        }
    }
    panic!("invalid private key")
}
