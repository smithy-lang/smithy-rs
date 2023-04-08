use aws_sdk_resourceexplorer2::output::SearchOutput;
use compiletime_benchmark::common_tag;
use tokio;

#[tokio::main]
async fn main() {
    let conf = aws_config::load_from_env().await;
    let cc_client = aws_sdk_cloudcontrol::Client::new(&conf);
    let re2_client = aws_sdk_resourceexplorer2::Client::new(&conf);
    let arn = {
        let res = re2_client
            .create_view()
            .view_name("smithy-rs-compiletime-benchmark")
            .set_tags(common_tag())
            .send()
            .await
            .unwrap();
        res.view().unwrap().view_arn().unwrap().to_string()
    };
    let target_arns = {
        let mut res: Option<SearchOutput> = None;
        let mut tasks = vec![];
        let mut target_arns = vec![];
        loop {
            let next_token = res
                .as_ref()
                .and_then(|i| i.next_token())
                .and_then(|i| i.to_string().into());
            let out = re2_client
                .search()
                .view_arn(&arn)
                .set_next_token(next_token)
                .send()
                .await
                .unwrap();

            let resources = out.resources().and_then(|r| Some(r.to_vec())).unwrap();
            for i in resources {
                target_arns.push(i.arn().unwrap().to_string());
                let cc_client = cc_client.clone();
                let task = tokio::spawn(async move {
                    println!("deleting {}", i.arn().unwrap());
                    let res = cc_client
                        .delete_resource()
                        .identifier(i.arn().unwrap())
                        .send()
                        .await
                        .unwrap();
                    let token = res.progress_event().unwrap();
                    let status = cc_client
                        .get_resource_request_status()
                        .request_token(token.identifier().unwrap())
                        .send()
                        .await;
                    res
                        .progress_event()
                        .unwrap()
                        .operation_status();
                });
                tasks.push(task);
            }
            if out.next_token().is_none() {
                break;
            };
            res.replace(out);
        }
        for i in tasks {
            i.await;
        }
        target_arns
    };

    {
        for arn in target_arns {}
    };
    re2_client.delete_view().view_arn(arn).send().await;
    {}
}
