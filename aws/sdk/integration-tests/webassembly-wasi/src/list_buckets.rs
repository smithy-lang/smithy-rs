/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::operation::list_buckets::ListBucketsOutput;

pub async fn s3_list_buckets() -> ListBucketsOutput {
    use aws_sdk_s3::Client;

    use crate::default_config::get_default_config;

    let shared_config = get_default_config(crate::adapter::Adapter::to_http_connector()).await;
    let client = Client::new(&shared_config);
    let result = client.list_buckets().send().await.unwrap();
    result
}

#[cfg(test)]
mod test {
    use crate::adapter::http_client::RESPONSE_BODY;
    use crate::default_config::get_default_config;
    use aws_sdk_s3::Client;
    use aws_smithy_client::test_connection::capture_request;
    use aws_smithy_http::body::SdkBody;
    use http::header::AUTHORIZATION;

    #[tokio::test]
    pub async fn test_s3_list_buckets() {
        let (conn, req) = capture_request(Some(
            http::Response::builder()
                .body(SdkBody::from(RESPONSE_BODY))
                .unwrap(),
        ));
        let shared_config = get_default_config(conn).await;
        let client = Client::new(&shared_config);
        let _result = client.list_buckets().send().await.unwrap();
        let req = req.expect_request();
        assert_eq!(req.uri().to_string(), "https://s3.us-west-2.amazonaws.com/");
        assert_eq!(
            req.headers()
                .get(AUTHORIZATION)
                .expect("should exist")
                .to_str()
                .unwrap(),
            "AWS4-HMAC-SHA256 Credential=access_key/20090213/us-west-2/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=a85e38189eca7104d0782e08434c628b73f68f7d0ba57e11958cf8ccf19f3759"
        );
    }
}
