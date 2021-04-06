use polly::model::{Engine, OutputFormat, VoiceId};
use std::error::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
    let client = polly::Client::from_env();
    let resp = client
        .synthesize_speech()
        .voice_id(VoiceId::Emma)
        .engine(Engine::Neural)
        .output_format(OutputFormat::Mp3)
        .text("Hello, I am polly! I generated this Audio with a neural network based TTS for the AWS SDK.")
        .send()
        .await?;
    let mut file = File::create("audio.mp3").await?;
    let mut audio = resp.audio_stream;
    let mut bytes_written = 0;
    while let Some(bytes) = audio.next().await {
        let bytes = bytes?;
        bytes_written += bytes.len();
        file.write_all(&bytes).await?;
    }
    println!("Audio written to audio.mp3 ({} bytes)", bytes_written);
    Ok(())
}
