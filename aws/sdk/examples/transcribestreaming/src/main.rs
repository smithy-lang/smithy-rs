/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use async_stream::stream;
use aws_sdk_transcribestreaming::model::{AudioEvent, AudioStream, LanguageCode, MediaEncoding};
use aws_sdk_transcribestreaming::{Blob, Client, Config, Region};
use bytes::BufMut;
use std::time::Duration;

const CHUNK_SIZE: usize = 8192;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let input_stream = stream! {
        let pcm = pcm_data();
        for chunk in pcm.chunks(CHUNK_SIZE) {
            // Sleeping isn't necessary, but emphasizes the streaming aspect of this
            tokio::time::sleep(Duration::from_millis(100)).await;
            yield Ok(AudioStream::AudioEvent(AudioEvent::builder().audio_chunk(Blob::new(chunk)).build()));
        }
        // Must send an empty chunk at the end
        yield Ok(AudioStream::AudioEvent(AudioEvent::builder().audio_chunk(Blob::new(Vec::new())).build()));
    };

    let config = Config::builder()
        .region(Region::from_static("us-west-2"))
        .build();
    let client = Client::from_conf(config);

    let mut output = client
        .start_stream_transcription()
        .language_code(LanguageCode::EnGb)
        .media_sample_rate_hertz(8000)
        .media_encoding(MediaEncoding::Pcm)
        .audio_stream(input_stream.into())
        .send()
        .await
        .unwrap();

    loop {
        match output.transcript_result_stream.recv().await {
            Ok(Some(transcription)) => {
                println!("Received transcription response:\n{:?}\n", transcription)
            }
            Ok(None) => break,
            Err(err) => println!("Received an error: {:?}", err),
        }
    }
    println!("Done.")
}

fn pcm_data() -> Vec<u8> {
    let audio = include_bytes!("../audio/hello-transcribe-8000.wav");
    let reader = hound::WavReader::new(&audio[..]).unwrap();
    let samples_result: hound::Result<Vec<i16>> = reader.into_samples::<i16>().collect();

    let mut pcm: Vec<u8> = Vec::new();
    for sample in samples_result.unwrap() {
        pcm.put_i16_le(sample);
    }
    pcm
}
