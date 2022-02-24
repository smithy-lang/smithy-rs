/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// Files here are for running integration tests.
// These tests only have access to your crate's public API.
// See: https://doc.rust-lang.org/book/ch11-03-test-organization.html#integration-tests

use std::{
    collections::HashMap,
    env::{self, current_dir},
    fmt,
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
    time::Duration,
};

use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use tokio::{process::Command, time};

use helpers::PokemonService;

#[macro_use]
mod helpers;

#[derive(Deserialize, Debug, PartialEq, Hash, Clone)]
struct WrkOptions {
    threads: u16,
    connections: u16,
    duration: Duration,
}

impl Eq for WrkOptions {}

impl WrkOptions {
    fn new(threads: u16, connections: u16, duration: u64) -> Self {
        Self {
            threads,
            connections,
            duration: Duration::from_secs(duration),
        }
    }
}

impl fmt::Display for WrkOptions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "threads: {} connections: {} duration: {} secs",
            self.threads,
            self.connections,
            self.duration.as_secs()
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct WrkResult {
    #[serde(skip)]
    pub success: bool,
    #[serde(skip)]
    pub error: String,
    pub requests: u64,
    pub errors: u64,
    pub successes: u64,
    pub requests_sec: u64,
    pub avg_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
    pub stdev_latency_ms: f64,
    pub transfer_mb: u64,
    pub errors_connect: usize,
    pub errors_read: usize,
    pub errors_write: usize,
    pub errors_status: usize,
    pub errors_timeout: usize,
}

impl WrkResult {
    fn fail(error: String) -> Self {
        Self {
            error,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
struct Wrk {
    pub url: String,
    pub lua: Option<PathBuf>,
    pub runs: HashMap<WrkOptions, WrkResult>,
    pub storage: PathBuf,
}

impl Wrk {
    fn new(url: &str, lua: Option<&str>, storage: Option<&str>) -> Self {
        Self {
            url: String::from(url),
            lua: lua.map(|x| Path::new(x).to_path_buf()),
            storage: storage
                .map(|x| Path::new(x).to_path_buf())
                .unwrap_or(current_dir().unwrap()),
            runs: HashMap::new(),
        }
    }

    async fn run(&mut self, options: WrkOptions, max_error_percentage: u16) {
        let mut command = format!(
            "wrk -t{} -c{} -d {}s ",
            options.threads,
            options.connections,
            options.duration.as_secs()
        );
        let mut wrk = Command::new("wrk");
        wrk.arg("-t")
            .arg(options.threads.to_string())
            .arg("-c")
            .arg(options.connections.to_string())
            .arg("-d")
            .arg(format!("{}s", options.duration.as_secs()));
        let wrk_command = match self.lua.as_ref() {
            Some(lua) => {
                if !lua.exists() {
                    println!(
                        "ERROR Wrk Lua file {} not found in {}",
                        env::current_dir().expect("unable to get current directory").display(),
                        lua.display()
                    );
                    return;
                }
                command += &format!("-s{}", lua.display());
                wrk.arg("-s").arg(lua).arg(&self.url)
            }
            None => wrk.arg(&self.url),
        };
        println!("Running '{}'", command);
        let run = match wrk_command.output().await {
            Ok(wrk) => {
                let output = String::from_utf8_lossy(&wrk.stdout);
                let error = String::from_utf8_lossy(&wrk.stderr);
                if wrk.status.success() {
                    println!("Wrk execution succeded:\n{}", output);
                    let wrk_json = output.split("JSON").nth(1).unwrap_or("{}");
                    match serde_json::from_str::<WrkResult>(wrk_json) {
                        Ok(mut run) => {
                            let error_percentage = run.errors / 100 * run.requests;
                            if error_percentage < max_error_percentage.into() {
                                run.success = true;
                            } else {
                                println!(
                                    "Errors percentage is {}%, which is more than {}%",
                                    error_percentage, max_error_percentage
                                );
                            }
                            run
                        }
                        Err(e) => {
                            println!("ERROR Wrk JSON result deserialize failed: {}", e);
                            WrkResult::fail(e.to_string())
                        }
                    }
                } else {
                    println!("ERROR Wrk execution failed.\nOutput: {}\nError: {}", output, error);
                    WrkResult::fail(error.to_string())
                }
            }
            Err(e) => {
                println!("ERROR Wrk execution failed: {}", e);
                WrkResult::fail(e.to_string())
            }
        };
        self.runs.insert(options, run);
    }

    fn dump(&self) -> Result<()> {
        let file = File::open(self.storage.join(".smithy-bench/old.json"))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(writer, &self.best()?)?;
        Ok(())
    }

    fn load(&self) -> Result<WrkResult> {
        let file = File::open(self.storage.join(".smithy-bench/old.json"))?;
        let mut reader = BufReader::new(file);
        Ok(serde_json::from_reader(&mut reader)?)
    }

    fn best(&self) -> Result<WrkResult> {
        let best = self.runs.iter().filter(|v| v.1.success).max_by(|a, b| {
            a.1.requests_sec
                .cmp(&b.1.requests_sec)
                .then(a.1.successes.cmp(&b.1.successes))
                .then(a.1.requests.cmp(&b.1.requests))
                .then(a.1.transfer_mb.cmp(&b.1.transfer_mb))
        });
        best.map(|x| x.1.clone())
            .ok_or(eyre!("Unable to calculate best run in set"))
    }

    fn compare(&self) -> Result<()> {
        let best = self.best()?;
        let old = self.load()?;
        Ok(())
    }
}

#[tokio::test]
async fn benchmark() -> Result<()> {
    let steps = vec![
        // WrkOptions::new(2, 64, 5),
        // WrkOptions::new(2, 128, 5),
        WrkOptions::new(2, 256, 5),
        // WrkOptions::new(4, 64, 5),
        // WrkOptions::new(4, 128, 5),
        WrkOptions::new(4, 256, 5),
        // WrkOptions::new(8, 64, 5),
        // WrkOptions::new(8, 128, 5),
        WrkOptions::new(8, 256, 5),
        // WrkOptions::new(12, 64, 5),
        // WrkOptions::new(12, 128, 5),
        WrkOptions::new(12, 256, 5),
    ];
    let _program = PokemonService::run();
    // Give PokemonService the time to start up.
    time::sleep(Duration::from_millis(500)).await;
    let mut wrk = Wrk::new("http://localhost:13734/pokemon-species/pikachu", Some("json.lua"), None);
    for step in steps {
        wrk.run(step, 5).await;
    }
    wrk.dump();
    Ok(())
}
