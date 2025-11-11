/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::mk_canary;
use anyhow::bail;

use aws_sdk_ec2 as ec2;
use aws_sdk_ec2::types::InstanceType;

use crate::CanaryEnv;

mk_canary!(
    "ec2_paginator",
    |sdk_config: &aws_config::SdkConfig, env: &CanaryEnv| {
        paginator_canary(ec2::Client::new(sdk_config), env.page_size)
    }
);

pub async fn paginator_canary(client: ec2::Client, page_size: usize) -> anyhow::Result<()> {
    let mut history = client
        .describe_spot_price_history()
        .instance_types(InstanceType::M1Medium)
        .into_paginator()
        .page_size(page_size as i32)
        .send();

    let mut num_pages = 0;
    while let Some(page) = history.try_next().await? {
        let items_in_page = page.spot_price_history.unwrap_or_default().len();
        if items_in_page > page_size {
            bail!(
                "failed to retrieve results of correct page size (expected {page_size}, got {items_in_page})",
            )
        }
        num_pages += 1;
    }
    if num_pages < 2 {
        bail!("expected 3+ pages containing ~60 results but got {num_pages} pages",)
    }

    // https://github.com/awslabs/aws-sdk-rust/issues/405
    let _ = client
        .describe_vpcs()
        .into_paginator()
        .items()
        .send()
        .collect::<Result<Vec<_>, _>>()
        .await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::paginator_canary;

    #[tokio::test]
    async fn test_paginator() {
        let conf = aws_config::load_from_env().await;
        let client = aws_sdk_ec2::Client::new(&conf);
        paginator_canary(client, 20).await.unwrap()
    }
}
