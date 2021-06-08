use chrono::{DateTime, Utc};

#[tokio::main]
async fn main() -> Result<(), sagemaker::Error> {
    let client = sagemaker::Client::from_env();
    let job_details = client.list_training_jobs().send().await?;

    println!("Job Name\tCreation DateTime\tDuration\tStatus");
    for j in job_details.training_job_summaries.unwrap_or_default() {
        let name = j.training_job_name.as_deref().unwrap_or_default();
        let creation_time_secs = j.creation_time.unwrap().epoch_seconds();
        let training_end_time_secs = j.training_end_time.unwrap().epoch_seconds();

        let native_creation_time_string =
            chrono::NaiveDateTime::from_timestamp(creation_time_secs, 0);

        let utc_creation_time: DateTime<Utc> =
            chrono::DateTime::from_utc(native_creation_time_string, chrono::Utc);

        let status = j.training_job_status.unwrap();
        let duration = training_end_time_secs - creation_time_secs;

        let deets = format!(
            "{}\t{}\t{}\t{:#?}",
            name,
            utc_creation_time.format("%Y-%m-%d@%H:%M:%S"),
            duration,
            status
        );
        println!("{}", deets);
    }

    Ok(())
}
