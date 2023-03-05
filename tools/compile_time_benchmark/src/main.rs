/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    path::{Path, PathBuf},
};

use aws_sdk_batch::model::JobDependency;
use serde::Serialize;
use tokio::{spawn, task::JoinError};

#[tokio::main]
fn main() {
    let conf = aws_config::load_from_env().await;
    
}

struct SaveData {
    id: String,
    save_directory: PathBuf,
    operations: Vec<String>,
}

impl SaveData {
    async fn write_log<T: Serialize>(
        &mut self,
        sub_dir: impl AsRef<Path>,
        file_name: impl AsRef<Path>,
        s: T,
    ) -> std::io::Result<()> {
        let mut path = self.save_directory.join(sub_dir.as_ref());
        path.set_file_name(file_name.as_ref());

        let contents = serde_json::to_string_pretty(&s).unwrap();
        tokio::fs::write(path, contents).await
    }

    async fn write_success_log<T: Serialize>(
        &mut self,
        file_name: impl AsRef<Path>,
        s: T,
    ) -> std::io::Result<()> {
        self.write_log("success", file_name, s).await
    }

    async fn write_failure_log<T: Serialize>(
        &mut self,
        file_name: impl AsRef<Path>,
        s: T,
    ) -> std::io::Result<()> {
        self.write_log("failure", file_name, s).await
    }
}

async fn create_passive_resources(
    client: aws_sdk_batch::Client,
    ec2: aws_sdk_ec2::Client,
    save_data: &mut SaveData,
) -> Result<(), JoinError> {
    let x = ec2.create_launch_template().send().await;
    let ce = tokio::spawn(client.create_compute_environment().send());
    let queue = tokio::spawn(client.create_job_queue().send());
    let definition = tokio::spawn(client.register_job_definition().send());
    Ok(())
}

async fn drop_batch_resources(client: aws_sdk_batch::Client) -> Result<(), JoinError> {
    let ce = tokio::spawn(client.delete_compute_environment().send());
    let queue = tokio::spawn(client.delete_job_queue().send());
    let definition = tokio::spawn(client.deregister_job_definition().send());
    Ok(())
}

async fn submit_job(client: aws_sdk_batch::Client) {
    if let Ok(job) = client.submit_job().send().await {
        client.submit_job().depends_on({
            JobDependency::builder().job_id(job.job_id().unwrap()).build()
        }).send().await;
    }
    let res = client.submit_job().send().await;
}
