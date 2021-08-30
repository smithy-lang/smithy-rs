/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use async_stream::stream;
use aws_sdk_transcribestreaming::error::{
    StartStreamTranscriptionError, StartStreamTranscriptionErrorKind,
};
use aws_sdk_transcribestreaming::model::{
    AudioEvent, AudioStream, LanguageCode, MediaEncoding, TranscriptResultStream,
};
use aws_sdk_transcribestreaming::output::StartStreamTranscriptionOutput;
use aws_sdk_transcribestreaming::{Blob, Client, Config, Credentials, Region, SdkError};
use bytes::BufMut;
use futures_core::Stream;
use smithy_client::dvr::{Event, ReplayingConnection};
use smithy_eventstream::frame::{HeaderValue, Message};
use smithy_http::event_stream::BoxError;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;

const CHUNK_SIZE: usize = 8192;

#[tokio::test]
async fn test_success() {
    let input_stream = stream! {
        let pcm = pcm_data();
        for chunk in pcm.chunks(CHUNK_SIZE) {
            yield Ok(AudioStream::AudioEvent(AudioEvent::builder().audio_chunk(Blob::new(chunk)).build()));
        }
    };
    let (replayer, mut output) =
        start_request("us-west-2", include_str!("success.json"), input_stream).await;

    let mut full_message = String::new();
    while let Some(event) = output.transcript_result_stream.recv().await.unwrap() {
        match event {
            TranscriptResultStream::TranscriptEvent(transcript_event) => {
                let transcript = transcript_event.transcript.unwrap();
                for result in transcript.results.unwrap_or_else(|| Vec::new()) {
                    if !result.is_partial {
                        let first_alternative = &result.alternatives.as_ref().unwrap()[0];
                        full_message += first_alternative.transcript.as_ref().unwrap();
                        full_message.push('\n');
                    }
                }
            }
            otherwise => panic!("received unexpected event type: {:?}", otherwise),
        }
    }

    // Validate the requests
    replayer
        .validate(&["content-type", "content-length"], validate_body)
        .await
        .unwrap();

    // Validate the responses
    assert_eq!(
        "Good day to you transcribe.\nThis is Polly talking to you from the Rust ST K.\n",
        full_message
    );
}

#[tokio::test]
async fn test_error() {
    let input_stream = stream! {
        let pcm = pcm_data();
        for chunk in pcm.chunks(CHUNK_SIZE).take(1) {
            yield Ok(AudioStream::AudioEvent(AudioEvent::builder().audio_chunk(Blob::new(chunk)).build()));
        }
    };
    let (replayer, mut output) =
        start_request("us-east-1", include_str!("error.json"), input_stream).await;

    match output.transcript_result_stream.recv().await {
        Err(SdkError::ServiceError {
            err:
                StartStreamTranscriptionError {
                    kind: StartStreamTranscriptionErrorKind::BadRequestException(err),
                    ..
                },
            ..
        }) => {
            assert_eq!(
                Some("A complete signal was sent without the preceding empty frame."),
                err.message()
            );
        }
        otherwise => panic!("Expected BadRequestException, got: {:?}", otherwise),
    }

    // Validate the requests
    replayer
        .validate(&["content-type", "content-length"], validate_body)
        .await
        .unwrap();
}

async fn start_request(
    region: &'static str,
    events_json: &str,
    input_stream: impl Stream<Item = Result<AudioStream, BoxError>> + Send + Sync + 'static,
) -> (ReplayingConnection, StartStreamTranscriptionOutput) {
    let events: Vec<Event> = serde_json::from_str(events_json).unwrap();
    let replayer = ReplayingConnection::new(events);

    let region = Region::from_static(region);
    let credentials = Credentials::from_keys("test", "test", None);
    let config = Config::builder()
        .region(region)
        .credentials_provider(credentials)
        .build();
    let client = Client::from_conf_conn(config, replayer.clone());

    let output = client
        .start_stream_transcription()
        .language_code(LanguageCode::EnGb)
        .media_sample_rate_hertz(8000)
        .media_encoding(MediaEncoding::Pcm)
        .audio_stream(input_stream.into())
        .send()
        .await
        .unwrap();

    (replayer, output)
}

fn validate_body(expected_body: &[u8], actual_body: &[u8]) -> Result<(), Box<dyn StdError>> {
    let expected_wrapper = Message::read_from(expected_body).unwrap();
    let expected_msg = Message::read_from(expected_wrapper.payload().as_ref()).unwrap();

    let actual_wrapper = Message::read_from(actual_body).unwrap();
    let actual_msg = Message::read_from(actual_wrapper.payload().as_ref()).unwrap();

    assert_eq!(
        header_names(&expected_wrapper),
        header_names(&actual_wrapper)
    );
    assert_eq!(header_map(&expected_msg), header_map(&actual_msg));
    assert_eq!(expected_msg.payload(), actual_msg.payload());
    Ok(())
}

fn header_names(msg: &Message) -> BTreeSet<String> {
    msg.headers()
        .iter()
        .map(|h| h.name().as_str().into())
        .collect()
}
fn header_map(msg: &Message) -> BTreeMap<String, &HeaderValue> {
    msg.headers()
        .iter()
        .map(|h| (h.name().as_str().to_string(), h.value()))
        .collect()
}

fn pcm_data() -> Vec<u8> {
    let audio = include_bytes!("hello-transcribe-8000.wav");
    let reader = hound::WavReader::new(&audio[..]).unwrap();
    let samples_result: hound::Result<Vec<i16>> = reader.into_samples::<i16>().collect();

    let mut pcm: Vec<u8> = Vec::new();
    for sample in samples_result.unwrap() {
        pcm.put_i16_le(sample);
    }
    pcm
}
