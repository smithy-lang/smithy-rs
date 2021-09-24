use anyhow::Context;
use aws_sdk_polly::{
    model::{Engine, OutputFormat, VoiceId},
};
use aws_sdk_transcribestreaming::{
    model::{LanguageCode, Media, MediaFormat},
};
use clap::ArgMatches;
use log::{debug, error, info};
use rodio::{Decoder, OutputStream, Sink};
use std::time::Duration;
use tempdir::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    runtime::Runtime,
};

/// While playing the telephone game, the user can pass an arg that defines how many times to pass the message through Polly and Transcribe.
/// This is the default number of iterations to do when the user doesn't specify.
const DEFAULT_NUMBER_OF_ITERATIONS: u32 = 5;
/// When running a job/task that takes some time to complete (speech synthesis and transcription), this sets a maximum wait time in seconds before giving up.
const TASK_TIMEOUT_IN_SECONDS: i32 = 30;
/// How often to poll for job/task status
const TASK_WAIT_INTERVAL_IN_SECONDS: i32 = 2;

fn main() {
    // Read ENV vars from a .env file if one exists
    // To see logs set RUST_LOG="error,telephone_game=debug"
    dotenv::dotenv().expect("failed to read env vars");
    // Start up the logger
    env_logger::init();
    let app = build_clap_app();

    match app.get_matches().subcommand() {
        Some(("play", matches)) => play_telephone(matches),
        Some(("parrot", matches)) => test_polly(matches),
        _ => (),
    }
}

fn build_clap_app<'app>() -> clap::App<'app> {
    clap::App::new("telephone-game")
        .version("0.1.0")
        .author("Zelda H. <zhessler@amazon.com>")
        .about("Does awesome things")
        .subcommand(
        clap::App::new("play")
                .about("Start playing a game of Telephone")
                .arg("--phrase=[PHRASE] 'The phrase to play the game with'")
                .arg("-i [iterations] 'The number of times to relay the telephone message, defaults to 1 when omitted'")
                .arg("-b [s3_bucket_name] 'The name of the S3 bucket that will be used to store intermediate audio and text files created by the game, defaults to telephone-game when omitted'")
        )
        .subcommand(clap::App::new("parrot").about("hear polly repeat what you say"))
}

/// Make Polly speak what you type
fn test_polly(_matches: &ArgMatches) {
    // Create a runtime so we can do some async Rust
    let rt = Runtime::new().expect("failed to create an async runtime");
    // Create a string to store user input
    let mut line = String::new();

    rt.block_on(async {
        // Create a new AWS Config
        let config = aws_config::load_from_env().await;
        let polly_client = aws_sdk_polly::Client::new(&config);

        // Set up a temp directory to store audio files
        let tmp_dir = TempDir::new("telephone-game").expect("couldn't create temp dir");
        let tmp_file_path = tmp_dir.path().join("polly.mp3");

        // Set up the ability to play audio
        let (_stream, stream_handle) =
            OutputStream::try_default().expect("Couldn't get handle to default audio output");
        let sink = Sink::try_new(&stream_handle).unwrap();

        loop {
            info!(r#"Say something and I'll echo it or say "quit" to quit"#);
            // Collect user input
            let mut input_lines = BufReader::new(tokio::io::stdin()).lines();

            // Reading a line from stdin will include the trailing newline
            // We don't want that so we trim it
            line = input_lines
                .next_line()
                .await
                .expect("encountered IO error when attempting to read next line")
                .expect("stream is closed")
                .trim()
                .to_owned();

            // If the user types "quit" then the game will end
            if line == "quit" {
                break;
            }

            // Start synthesizing speech and wait for it to finish
            let res = polly_client
                .synthesize_speech()
                .text(&line)
                .voice_id(VoiceId::Joanna)
                .output_format(OutputFormat::Mp3)
                .send()
                .await;

            match res {
                Ok(res) => {
                    info!(r#"You said: "{}""#, &line);
                    info!("Playing Polly's response...");

                    // Collect the ByteStream returned by the synthesize_speech call
                    let byte_stream = res
                        .audio_stream
                        .collect()
                        .await
                        .expect("audio stream ended prematurely");

                    // Create a file to store the audio
                    let mut tmp_file = tokio::fs::File::create(&tmp_file_path)
                        .await
                        .expect("couldn't create temp file");
                    // Write the ByteStream to the file
                    tmp_file
                        .write_all(&byte_stream.into_bytes())
                        .await
                        .expect("couldn't write to temp file");
                    // Flush the write operation to ensure it finishes before we continue
                    tmp_file.flush().await.expect("couldn't flush");

                    // Open the audio file with regular blocking IO File
                    // rodio's Decoder requires stdlib Files
                    let file = std::fs::File::open(&tmp_file_path).unwrap();
                    let source =
                        Decoder::new(std::io::BufReader::new(file)).expect("couldn't decode audio");

                    // Set rodio to play the audio we just decoded
                    sink.append(source);
                    sink.sleep_until_end();

                    info!("Did you hear it?");
                }
                Err(e) => error!("{}", e),
            };

            // Clear last iteration's line so we can receive new user input
            line.clear();
        }
    })
}


/**
# Play a game of Telephone w/ AWS

1. Fetch user any user input that will override default values
1. Create a config and required clients for AWS services
1. Create a bucket to store audio and transcriptions if none exists
1. Run in a loop
    1. Start a speech synthesis task and set it to output to the previously created S3 bucket
    1. Wait up to TASK_TIMEOUT_IN_SECONDS seconds for synthesis task to complete
        - The status of the task is checked every TASK_WAIT_INTERVAL_IN_SECONDS in a loop
        - Break out of the loop once the task succeeds or fails
    1. Delete any pre-existing transcription job with the name "telephone-game-transcription"
        - Job names must be unique so we clear the old job to reuse the name.
    1. Wait up to TASK_TIMEOUT_IN_SECONDS seconds for transcripion job to complete
        - The status of the job is checked every TASK_WAIT_INTERVAL_IN_SECONDS in a loop
        - Break out of the loop once the job succeeds or fails
    1. Download the transcript JSON file from S3 and print what Transcribe heard
    1. Break out of the loop if an error occurs or we reach the iteration limit
1. Log a final message before closing
    - If everything succeeded, log the original phrase, the current phrase, and how many iterations occurred
    - If a failure occurs, log the error and any errors that caused it
*/
fn play_telephone(matches: &ArgMatches) {
    let rt = Runtime::new().expect("failed to create an async runtime");
    let number_of_iterations = matches
        .value_of("iterations")
        .and_then(|i| i.parse::<u32>().ok())
        .unwrap_or(DEFAULT_NUMBER_OF_ITERATIONS);

    if number_of_iterations == 0 {
        error!("Iterations must be a number greater than 0");
        return;
    }

    let original_phrase = matches.value_of("phrase").unwrap_or_default();
    let mut current_phrase = original_phrase.to_owned();

    let bucket_name = matches
        .value_of("s3_bucket_name")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "telephone-game".to_owned());

    let result = rt.block_on(async {
        let config = aws_config::load_from_env().await;
        let s3_client = aws_sdk_s3::Client::new(&config);
        let polly_client = aws_sdk_polly::Client::new(&config);
        let transcribe_client = aws_sdk_transcribestreaming::Client::new(&config);
        let bucket_name = create_s3_bucket_if_not_exists(&s3_client, &bucket_name)
            .await
            .context("Failed to complete necessary setup")?;

        let mut i = 0;
        'game: loop {
            debug!(
                "starting speech synthesis task for phrase '{}'",
                &current_phrase
            );

            let mut synthesis_task = polly_client
                .start_speech_synthesis_task()
                .text(&current_phrase)
                .voice_id(VoiceId::Joanna)
                .output_format(OutputFormat::Mp3)
                .output_s3_bucket_name(&bucket_name)
                .engine(Engine::Standard)
                .send()
                .await
                .context("Failed to start speech synthesis task")?
                .synthesis_task
                .unwrap();

            debug!(
                "Waiting for speech synthesis task to complete. Timeout is {}s",
                TASK_TIMEOUT_IN_SECONDS
            );
            let mut speech_synthesis_timeout_in_seconds = TASK_TIMEOUT_IN_SECONDS;

            'synthesis_task: loop {
                speech_synthesis_timeout_in_seconds -= TASK_WAIT_INTERVAL_IN_SECONDS;
                tokio::time::sleep(Duration::from_secs(TASK_WAIT_INTERVAL_IN_SECONDS as u64)).await;
                synthesis_task = polly_client
                    .get_speech_synthesis_task()
                    .task_id(synthesis_task.task_id.unwrap())
                    .send()
                    .await
                    .context("Failed to check status of speech synthesis task")?
                    .synthesis_task
                    .unwrap();

                use aws_sdk_polly::model::TaskStatus;
                match synthesis_task.task_status.unwrap() {
                    TaskStatus::Completed => {
                        debug!("Speech synthesis task completed");
                        break 'synthesis_task;
                    }
                    TaskStatus::Failed => {
                        let reason = synthesis_task
                            .task_status_reason
                            .unwrap_or_else(|| "(no reason given)".to_owned());
                        break 'game Err(anyhow::anyhow!(
                            "Speech synthesis task failed with reason: {}",
                            reason
                        ));
                    }
                    TaskStatus::InProgress | TaskStatus::Scheduled => {
                        debug!("Speech synthesis is ongoing...")
                    }
                    _ => unreachable!(),
                }

                if speech_synthesis_timeout_in_seconds <= 0 {
                    break 'game Err(anyhow::anyhow!(
                        "Speech synthesis task didn't complete before the {}s timeout elapsed",
                        TASK_TIMEOUT_IN_SECONDS
                    ));
                }
            }

            let output_uri = synthesis_task.output_uri.unwrap();
            let media = Media::builder().media_file_uri(&output_uri).build();

            debug!("Clearing pre-existing transcription job");
            match transcribe_client
                .delete_transcription_job()
                .transcription_job_name("telephone-game-transcription")
                .send()
                .await
            {
                Ok(_) => debug!("Previous transcription job deleted"),
                Err(e) => debug!("No previous transcription exists {}", e),
            };

            let mut transcription_job = transcribe_client
                .start_transcription_job()
                .transcription_job_name("telephone-game-transcription")
                .media_format(MediaFormat::Mp3)
                .language_code(LanguageCode::EnUs)
                .media(media)
                .output_bucket_name(&bucket_name)
                .send()
                .await
                .context("Failed to start transcription job")?
                .transcription_job
                .unwrap();

            debug!(
                "Waiting for transcription job to complete. Timeout is {}s",
                TASK_TIMEOUT_IN_SECONDS
            );
            let mut transcription_job_timeout_in_seconds = TASK_TIMEOUT_IN_SECONDS;

            'transcription_job: loop {
                transcription_job_timeout_in_seconds -= TASK_WAIT_INTERVAL_IN_SECONDS;
                tokio::time::sleep(Duration::from_secs(TASK_WAIT_INTERVAL_IN_SECONDS as u64)).await;
                transcription_job = transcribe_client
                    .get_transcription_job()
                    .transcription_job_name(transcription_job.transcription_job_name.unwrap())
                    .send()
                    .await
                    .context("Failed to check status of transcription job")?
                    .transcription_job
                    .unwrap();

                use aws_sdk_transcribe::model::TranscriptionJobStatus;
                match transcription_job.transcription_job_status.unwrap() {
                    TranscriptionJobStatus::Completed => {
                        debug!("Transcription job completed");
                        break 'transcription_job;
                    }
                    TranscriptionJobStatus::Failed => {
                        let reason = transcription_job
                            .failure_reason
                            .unwrap_or_else(|| "(no reason given)".to_owned());
                        break 'game Err(anyhow::anyhow!(
                            "Transcription job failed with reason: {}",
                            reason
                        ));
                    }
                    TranscriptionJobStatus::InProgress | TranscriptionJobStatus::Queued => {
                        debug!("Transcription job is ongoing...")
                    }
                    TranscriptionJobStatus::Unknown(_) | _ => unreachable!(),
                }

                if transcription_job_timeout_in_seconds <= 0 {
                    break 'game Err(anyhow::anyhow!(
                        "Transcription job didn't complete before the {}s timeout elapsed",
                        TASK_TIMEOUT_IN_SECONDS
                    ));
                }
            }

            let transcript = s3_client
                .get_object()
                .bucket(&bucket_name)
                .key("telephone-game-transcription.json")
                .send()
                .await
                .context("Failed to get transcript from S3")?
                .body
                .collect()
                .await
                .context("Failed to collect ByteStream")?
                .into_bytes();

            let transcript = std::str::from_utf8(&transcript)
                .context("Failed to parse transcript as UTF-8 text")?;
            let transcript: serde_json::Value =
                serde_json::from_str(transcript).context("Failed to parse transcript as JSON")?;

            let transcript = transcript["results"]["transcripts"][0]["transcript"]
                .as_str()
                .unwrap()
                .to_owned();

            info!("Transcription #{} == {}", i, &transcript);
            current_phrase = transcript;

            i += 1;

            if i == number_of_iterations {
                break 'game Ok(());
            }
        }
    });



    match result {
        Ok(()) => {
            info!(
r#"The phrase
"{}"
became
"{}"
after {} iterations
"#, original_phrase, current_phrase, number_of_iterations
                );
        },
        Err(e) => {
            let error_chain: String = e
                .chain()
                // We skip the first error so it doesn't get printed twice
                .skip(1)
                .map(|e| format!("Caused by:\n\t{}\n", e))
                .collect();
            let full_error_message = format!("Encountered an error: {}\n{}", e, error_chain);

            error!("{}", full_error_message);
        }
    }
}

/// Check if a bucket exists and create one if it doesn't. Then, return the bucket's name.
async fn create_s3_bucket_if_not_exists(
    s3_client: &s3::Client,
    bucket_name: &str,
) -> anyhow::Result<String> {
    let bucket_list = s3_client
        .list_buckets()
        .send()
        .await
        .context("Failed to list buckets when checking for existing bucket")?;
    let maybe_existing_bucket = bucket_list.buckets.unwrap().into_iter().find(|bucket| {
        bucket
            .name
            .as_ref()
            .map(|name| name == bucket_name)
            .unwrap_or_default()
    });

    if let Some(_bucket) = maybe_existing_bucket {
        debug!("A bucket named '{}' already exists", bucket_name);
        Ok(bucket_name.to_owned())
    } else {
        debug!("Creating an S3 bucket to store intermediate text and audio files");
        s3_client
            .create_bucket()
            .bucket(bucket_name)
            .send()
            .await
            .map(|_| {
                debug!("Created new bucket '{}'", bucket_name);
                bucket_name.to_owned()
            })
            .with_context(|| format!("Failed to create new bucket '{}'", bucket_name))
    }
}
