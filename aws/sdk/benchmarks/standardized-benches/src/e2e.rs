/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::{Arc, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, System};
use tokio::task::JoinHandle;

pub mod benchmark_types;
pub mod ddb;
pub mod s3;

pub struct ResourceMonitor {
    handle: JoinHandle<()>,
    cpu_samples: Arc<Mutex<Vec<f64>>>,
    memory_samples: Arc<Mutex<Vec<f64>>>,
}

impl ResourceMonitor {
    pub fn spawn(interval_ms: u64) -> Self {
        let cpu_samples = Arc::new(Mutex::new(Vec::new()));
        let memory_samples = Arc::new(Mutex::new(Vec::new()));
        let cpu = cpu_samples.clone();
        let mem = memory_samples.clone();

        let handle = tokio::spawn(async move {
            let mut sys = System::new();
            let pid = Pid::from_u32(std::process::id());
            sys.refresh_process_specifics(pid, ProcessRefreshKind::new().with_cpu());
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            loop {
                sys.refresh_process_specifics(
                    pid,
                    ProcessRefreshKind::new().with_cpu().with_memory(),
                );
                if let Some(process) = sys.process(pid) {
                    cpu.lock()
                        .expect("cpu lock poisoned")
                        .push(process.cpu_usage() as f64);
                    mem.lock()
                        .expect("mem lock poisoned")
                        .push(process.memory() as f64 / 1024.0 / 1024.0);
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
            }
        });

        Self {
            handle,
            cpu_samples,
            memory_samples,
        }
    }

    pub fn stop(self) -> (Vec<f64>, Vec<f64>) {
        self.handle.abort();
        let cpu = self.cpu_samples.lock().expect("cpu lock poisoned").clone();
        let mem = self
            .memory_samples
            .lock()
            .expect("mem lock poisoned")
            .clone();
        (cpu, mem)
    }
}
