use std::error::Error;
use polly::model::{VoiceId, OutputFormat};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = polly::fluent::Client::from_env();
    let resp = client.synthesize_speech()
        .voice_id(VoiceId::Carla)
        .output_format(OutputFormat::Mp3)
        .text("hello, I am polly!").send().await?;
    let audio = resp.audio_stream.expect("data should be included");
    let mut file = File::create("audio.mp3").await?;
    file.write_all(audio.as_ref()).await?;
    println!("Audio written to audio.mp3");
    Ok(())
}
