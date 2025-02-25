/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
use std::{env, fs};

use aws_config::timeout::TimeoutConfig;
use aws_credential_types::Credentials;
use aws_sdk_sts::error::SdkError;

use aws_smithy_http_client::tls::{self, TlsContext, TrustStore};
use aws_smithy_http_client::Builder as HttpClientBuilder;

use rustls::pki_types::CertificateDer;
#[cfg(debug_assertions)]
use x509_parser::prelude::*;

const OPERATION_TIMEOUT: u64 = 5;

fn unsupported() {
    println!("UNSUPPORTED");
    std::process::exit(exitcode::OK);
}

fn get_credentials() -> Credentials {
    Credentials::from_keys(
        "AKIAIOSFODNN7EXAMPLE",
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        None,
    )
}

#[cfg(debug_assertions)]
fn debug_cert(cert: &[u8]) {
    let x509 = X509Certificate::from_der(cert).unwrap();
    let subject = x509.1.subject();
    let serial = x509.1.raw_serial_as_string();
    println!("Adding root CA: {subject} ({serial})");
}

fn add_cert_to_store(cert: &[u8], store: &mut rustls::RootCertStore) {
    #[cfg(debug_assertions)]
    debug_cert(cert);
    let der = CertificateDer::from_slice(cert);
    if let Err(e) = store.add(der) {
        println!("Error adding root certificate: {e}");
        unsupported();
    }
}

fn load_ca_bundle(filename: &String) -> TrustStore {
    // test the certs are supported and fail quickly if not
    match File::open(filename) {
        Ok(f) => {
            let mut f = BufReader::new(f);
            let mut roots = rustls::RootCertStore::empty();
            match rustls_pemfile::certs(&mut f) {
                Ok(certs) => {
                    for cert in certs {
                        add_cert_to_store(&cert, &mut roots);
                    }
                }
                Err(e) => {
                    println!("Error reading PEM file: {e}");
                    unsupported();
                }
            }
        }
        Err(e) => {
            println!("Error opening file '{filename}': {e}");
            unsupported();
        }
    }

    let pem_bytes = fs::read(filename).expect("bundle exists");
    TrustStore::empty().with_pem_certificate(pem_bytes.as_slice())
}

fn load_native_certs() -> TrustStore {
    // why do we need a custom trust store when invoked with native certs?
    // Because the trytls tests don't actually only rely on native certs.
    // It's native certs AND the one replaced in the blank pem cert below
    // via the `update-certs` script.
    let pem_ca_cert = b"\
-----BEGIN CERTIFICATE-----
-----END CERTIFICATE-----\
" as &[u8];

    TrustStore::default().with_pem_certificate(pem_ca_cert)
}

async fn create_client(
    trust_store: TrustStore,
    host: &String,
    port: &String,
) -> aws_sdk_sts::Client {
    let credentials = get_credentials();
    let tls_context = TlsContext::new().with_trust_store(trust_store);
    let http_client = HttpClientBuilder::new()
        .tls_provider(tls::Provider::rustls(
            tls::rustls_provider::CryptoMode::AwsLc,
        ))
        .tls_context(tls_context)
        .build_https();
    let sdk_config = aws_config::from_env()
        .http_client(http_client)
        .credentials_provider(credentials)
        .region("us-nether-1")
        .endpoint_url(format!("https://{host}:{port}"))
        .timeout_config(
            TimeoutConfig::builder()
                .operation_timeout(Duration::from_secs(OPERATION_TIMEOUT))
                .build(),
        )
        .load()
        .await;
    aws_sdk_sts::Client::new(&sdk_config)
}

#[tokio::main]
async fn main() -> Result<(), aws_sdk_sts::Error> {
    let argv: Vec<String> = env::args().collect();
    if argv.len() < 3 || argv.len() > 4 {
        eprintln!("Syntax: {} <hostname> <port> [ca-file]", argv[0]);
        std::process::exit(exitcode::USAGE);
    }
    let trust_store = if argv.len() == 4 {
        print!(
            "Connecting to https://{}:{} with root CA bundle from {}: ",
            &argv[1], &argv[2], &argv[3]
        );
        load_ca_bundle(&argv[3])
    } else {
        print!(
            "Connecting to https://{}:{} with native roots: ",
            &argv[1], &argv[2]
        );
        load_native_certs()
    };

    let sts_client = create_client(trust_store, &argv[1], &argv[2]).await;
    match sts_client.get_caller_identity().send().await {
        Ok(_) => println!("\nACCEPT"),
        Err(SdkError::DispatchFailure(e)) => println!("{e:?}\nREJECT"),
        Err(SdkError::ServiceError(e)) => println!("{e:?}\nACCEPT"),
        Err(e) => {
            println!("Unexpected error: {e:#?}");
            std::process::exit(exitcode::SOFTWARE);
        }
    }
    Ok(())
}
