/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    path::{Path, PathBuf},
};

use aws_sdk_ec2::model::TagSpecification;
use serde::Serialize;
use tokio::{spawn, task::JoinError};

const TOGGLE_DRY_RUN: bool = env!("IS_DRY_RUN") == "FALSE";
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

macro_rules! ec2_request {
    ($client: ident, $target: ident) => {
        let file = include_str!(concat!("./config/ec2/", stringify!($target)));
        $client.$target().set_fields(file).send()
    };
}

async fn setup_resources() -> Result<(), JoinError> {
    let conf = aws_config::load_from_env().await;
    let tags = {
        include_str!("../config/common/tag.toml")
    };
    // ec2
    {
        let client_ec2 = aws_sdk_ec2::Client::new(&conf);
        let id = client_ec2.create_security_group().set_fields(include_str!("./config/ec2/create_security_group.toml")).send().await.unwrap().group_id();
        client_ec2.authorize_security_group_egress().group_id(id).tag_specifications(TagSpecification::builder().resource_type(aws_sdk_ec2::model::ResourceType::SecurityGroupRule).build()).send().await.unwrap();
        client_ec2.create_key_pair().set_fields(include_str!("./config/ec2/create_key_pair.toml")).send().await;
        client_ec2.create_launch_template().set_tag_specifications(input).set_fields(include_str!("./config/ec2/create_launch_template.toml")).send().await;
    };

    {
        let client_batch = aws_sdk_batch::Client::new(&conf);
        client_batch.register_job_definition().set_fields();
        client.create_compute_environment().send();
        client.create_job_queue().send();
        client.register_job_definition().send();
    };


    client_batch.submit_job();

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
