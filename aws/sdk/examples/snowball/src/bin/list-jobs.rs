/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#[tokio::main]
async fn main() -> Result<(), aws_sdk_snowball::Error> {
    let client = aws_sdk_snowball::Client::from_env();
    let jobs = client.list_jobs().send().await?;
    for job in jobs.job_list_entries {
        println!("JobId: {:?}", job.job_id);
    }

    Ok(())
}
