use tokio;

#[tokio::main]
async fn main() {
    let arn = "";
    let conf = aws_config::load_from_env().await;
    let cc_client = aws_sdk_cloudcontrol::Client::new(&conf);
    let re2_client = aws_sdk_resourceexplorer2::Client::new(&conf);
    
    let mut res = re2_client.search().view_arn(arn).send().await.unwrap();
    for i in res.resources().unwrap() {
        cc_client.delete_resource().identifier(i.arn().unwrap()).send();
    }
    loop {
        res = re2_client.search().set_next_token(res.next_token().and_then(|i|i.to_string().into() )).send().await.unwrap();
        for i in res.resources().unwrap() {
            cc_client.delete_resource().identifier(i.arn().unwrap()).send();
        }
        if res.next_token().is_none() {
            break
        };
    };
    cc_client.delete_view();
    {}
}