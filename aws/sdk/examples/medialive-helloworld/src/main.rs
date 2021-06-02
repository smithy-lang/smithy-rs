#[tokio::main]
async fn main() -> Result<(), medialive::Error> {
    let client = medialive::Client::from_env();
    let input_list = client.list_inputs().send().await?;

    match input_list.inputs {
        Some(inputs) => inputs.iter().for_each(|i| {
            let input_arn = i.arn.to_owned().unwrap_or_default();
            let input_name = i.name.to_owned().unwrap_or_default();

            println!("Input Name : {}, Input ARN : {}", input_name, input_arn);
        }),
        _ => (),
    }
    Ok(())
}
