/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! TLS related types for Python.
//!
//! [PyTlsConfig] implementation is mostly borrowed from:
//! <https://github.com/seanmonstar/warp/blob/4e9c4fd6ce238197fd1088061bbc07fa2852cb0f/src/tls.rs>

use std::fs::File;
use std::io::{self, BufReader, Read};

use pyo3::{pyclass, pymethods};
use thiserror::Error;
use tokio_rustls::rustls::{Certificate, Error as RustTlsError, PrivateKey, ServerConfig};

/// PyTlsConfig represents TLS configuration created from Python.
#[pyclass(name = "TlsConfig", text_signature = "(*, key_path, cert_path)")]
#[derive(Clone)]
pub struct PyTlsConfig {
    /// Absolute path of the DER-encoded RSA or PKCS private key.
    key_path: String,

    /// Absolute path of the DER-encoded x509 certificate.
    cert_path: String,
}

impl PyTlsConfig {
    /// Build [ServerConfig] from [PyTlsConfig].
    pub fn build(self) -> Result<ServerConfig, PyTlsConfigError> {
        let cert_chain = self.cert_chain()?;
        let key_der = self.key_der()?;
        let mut config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key_der)?;
        config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];
        Ok(config)
    }

    /// Reads certificates from `cert_path`.
    fn cert_chain(&self) -> Result<Vec<Certificate>, PyTlsConfigError> {
        let file = File::open(&self.cert_path).map_err(PyTlsConfigError::CertParse)?;
        let mut cert_rdr = BufReader::new(file);
        Ok(rustls_pemfile::certs(&mut cert_rdr)
            .map_err(PyTlsConfigError::CertParse)?
            .into_iter()
            .map(Certificate)
            .collect())
    }

    /// Parses RSA or PKCS private key from `key_path`.
    fn key_der(&self) -> Result<PrivateKey, PyTlsConfigError> {
        let mut key_vec = Vec::new();
        File::open(&self.key_path)
            .and_then(|mut f| f.read_to_end(&mut key_vec))
            .map_err(PyTlsConfigError::KeyParse)?;
        if key_vec.is_empty() {
            return Err(PyTlsConfigError::EmptyKey);
        }

        let mut pkcs8 = rustls_pemfile::pkcs8_private_keys(&mut key_vec.as_slice())
            .map_err(PyTlsConfigError::Pkcs8Parse)?;
        if !pkcs8.is_empty() {
            return Ok(PrivateKey(pkcs8.remove(0)));
        }

        let mut rsa = rustls_pemfile::rsa_private_keys(&mut key_vec.as_slice())
            .map_err(PyTlsConfigError::RsaParse)?;
        if !rsa.is_empty() {
            return Ok(PrivateKey(rsa.remove(0)));
        }

        Err(PyTlsConfigError::EmptyKey)
    }
}

#[pymethods]
impl PyTlsConfig {
    #[new]
    fn py_new(key_path: String, cert_path: String) -> Self {
        Self {
            key_path,
            cert_path,
        }
    }
}

/// Possible TLS configuration errors.
#[derive(Error, Debug)]
pub enum PyTlsConfigError {
    #[error("could not parse certificate")]
    CertParse(io::Error),
    #[error("could not parse key")]
    KeyParse(io::Error),
    #[error("empty key")]
    EmptyKey,
    #[error("could not parse pkcs8 keys")]
    Pkcs8Parse(io::Error),
    #[error("could not parse rsa keys")]
    RsaParse(io::Error),
    #[error("rusttls protocol error")]
    RustTlsError(#[from] RustTlsError),
}

#[cfg(test)]
mod tests {
    use pyo3::{
        prelude::*,
        types::{IntoPyDict, PyDict},
    };

    use super::*;

    #[test]
    fn creating_tls_config_in_python() -> PyResult<()> {
        pyo3::prepare_freethreaded_python();

        let config = Python::with_gil(|py| {
            let globals = [("TlsConfig", py.get_type::<PyTlsConfig>())].into_py_dict(py);
            let locals = PyDict::new(py);
            py.run(
                r#"
config = TlsConfig(key_path="key.rsa", cert_path="cert.pem")
"#,
                Some(globals),
                Some(locals),
            )?;
            locals.get_item("config").unwrap().extract::<PyTlsConfig>()
        })?;

        assert_eq!("key.rsa", config.key_path);
        assert_eq!("cert.pem", config.cert_path);

        Ok(())
    }

    #[test]
    fn building_server_config_from_tls_config() {
        const TEST_KEY: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/pokemon-service-test/tests/testdata/localhost.key"
        );
        const TEST_CERT: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/pokemon-service-test/tests/testdata/localhost.crt"
        );

        let tls_config = PyTlsConfig::py_new(TEST_KEY.to_string(), TEST_CERT.to_string());
        tls_config.build().unwrap();
    }
}
