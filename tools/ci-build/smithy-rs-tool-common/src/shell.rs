/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! In general, the `crate::command` module should be preferred over this one going forward.
//!
//! This module was an attempt to make unit testing against command-line tools easier (especially
//! in regards to git), but over time we've been realizing it's easier to just set up fake git
//! repositories to run real commands against, or to mock out individual pieces of functionality
//! where needed rather than individual commands. This module requires a ton of boilerplate for
//! what is providing.

use anyhow::Result;
use async_trait::async_trait;
use std::process::Output;

#[async_trait]
pub trait ShellOperation {
    type Output: Send + 'static;

    /// Runs the command synchronously.
    fn run(&self) -> Result<Self::Output>;

    /// Runs the command asynchronously.
    #[cfg(feature = "async")]
    async fn spawn(self) -> Result<Self::Output>
    where
        Self: Sized + 'static,
    {
        tokio::task::spawn_blocking(move || self.run()).await?
    }
}

/// Returns (stdout, stderr)
pub fn output_text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

pub fn handle_failure(operation_name: &str, output: &Output) -> Result<(), anyhow::Error> {
    if !output.status.success() {
        return Err(capture_error(operation_name, output));
    }
    Ok(())
}

pub fn capture_error(operation_name: &str, output: &Output) -> anyhow::Error {
    let message = format!(
        "Failed to {name}:\nStatus: {status}\nStdout: {stdout}\nStderr: {stderr}\n",
        name = operation_name,
        status = if let Some(code) = output.status.code() {
            format!("{code}")
        } else {
            "Killed by signal".to_string()
        },
        stdout = String::from_utf8_lossy(&output.stdout),
        stderr = String::from_utf8_lossy(&output.stderr)
    );
    anyhow::Error::msg(message)
}
