use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = polly::fluent::Client::from_env();
    let mut tok = None;
    // Below is an an example of how pagination can be implemented manually.
    loop {
        let mut req = client.describe_voices();
        if let Some(tok) = tok {
            req = req.next_token(tok);
        }
        let voices = req.send().await?;
        for voice in voices.voices.unwrap_or_default() {
            println!(
                "I can speak as: {} in {:?}",
                voice.name.unwrap(),
                voice.language_name.unwrap()
            );
        }
        tok = match voices.next_token {
            Some(next) => Some(next),
            None => break,
        };
    }
    Ok(())
}
