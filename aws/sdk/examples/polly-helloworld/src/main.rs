use polly::model::{Engine, Voice};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let client = polly::fluent::Client::from_env();
    let mut tok = None;
    let mut voices: Vec<Voice> = vec![];
    // Below is an an example of how pagination can be implemented manually.
    loop {
        let mut req = client.describe_voices();
        if let Some(tok) = tok {
            req = req.next_token(tok);
        }
        let resp = req.send().await?;
        for voice in resp.voices.unwrap_or_default() {
            println!(
                "I can speak as: {} in {:?}",
                voice.name.as_ref().unwrap(),
                voice.language_name.as_ref().unwrap()
            );
            voices.push(voice);
        }
        tok = match resp.next_token {
            Some(next) => Some(next),
            None => break,
        };
    }

    println!(
        "Voices supporting a neural engine: {:?}",
        voices
            .iter()
            .filter(|voice| voice
                .supported_engines
                .as_deref()
                .unwrap_or_default()
                .contains(&Engine::Neural))
            .map(|voice| voice.id.as_ref().unwrap())
            .collect::<Vec<_>>()
    );
    Ok(())
}
