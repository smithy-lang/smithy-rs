use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>>{
    let client = polly::fluent::Client::from_env();
    let mut tok = None;
    // manually paginate
    loop {
        let mut req = client.describe_voices();
        if let Some(tok) = tok {
            req = req.next_token(tok);
        }
        let voices = req.send().await?;
        for voice in voices.voices.unwrap_or_default() {
            println!("I can speak as: {} ({:?}) in these languages: {:?}", voice.name.unwrap(), voice.gender.unwrap(), voice.language_name.unwrap());
        }
        if voices.next_token == None {
            break
        } else {
            tok = voices.next_token;
        }
    }
    Ok(())
}
