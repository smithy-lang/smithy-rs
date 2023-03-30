
 use std::{
    convert::Infallible,
    path::{Path, PathBuf}, collections::HashMap,
};

use aws_sdk_batch::model::ComputeEnvironmentOrder;
use aws_sdk_ec2::model::{TagSpecification, Tag};
use compiletime_benchmark::force_dry_run;
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use tokio::{spawn, task::JoinError};

macro_rules! base_request {
    ($client: ident, ec2, $($target:ident)*) => {
        (
            $(
                {
                    let file = include_str!(concat!("../../config/ec2/",stringify!($target), ".toml"));
                    let toml_map = toml::from_str(file).unwrap();
                    $client.$target().dry_run(force_dry_run()).tag_specifications(toml_map)
                },
            )*
        )
    };
    ($client: ident, $service: ident, $($target:ident)*) => {
        (
            $(
                {
                    let file = include_str!(concat!("../../config/", stringify!($service),"/",stringify!($target), ".toml"));
                    let toml_map = toml::from_str(file).unwrap();
                    $client.$target().set_dry_run(force_dry_run()).set_tags(common_tag())
                },
            )*
        )
    };
}

macro_rules! ec2_request {
    ($client:ident, $($target:ident),*) => {
        base_request!($client, ec2, $($target)*)
    };
}

macro_rules! batch_request {
    ($client:ident, $($target:ident),*) => {
        base_request!($client, batch, $($target)*)
    };
}

macro_rules! iam_request {
    ($client:ident, $($target:ident),*) => {
        base_request!($client, iam, $($target)*)
    };
}

const DRY_RUN_FLAG_IS_SET: &'static str = "Request would have succeeded, but DryRun flag is set.";

fn common_tag<T: DeserializeOwned>() -> T {
    toml::from_str(include_str!("../../config/common/tag.toml")).unwrap()
}

#[tokio::main]
async fn main() -> Result<(), JoinError> {
    let conf = aws_config::load_from_env().await;
    let tags: HashMap<String, String> = {
        toml::from_str(include_str!("../../config/common/tag.toml")).unwrap()
    };
    
    {
        let client = aws_sdk_ec2::Client::new(&conf);
        let (
            create_launch_template ,
            create_subnet ,
            authorize_security_group_egress ,
            create_key_pair ,
            create_security_group
        ) = ec2_request!(client, 
            create_launch_template ,
            create_subnet ,        
            authorize_security_group_egress ,
            create_key_pair ,
            create_security_group);
    
        create_launch_template.send();
        create_subnet.send();
        create_key_pair.send();
        create_security_group.send();
        
    };
    
    {
        let batch = aws_sdk_batch::Client::new(&conf);
        let (register_job_definition, create_compute_environment, create_job_queue) = batch_request!(batch, register_job_definition, create_compute_environment, create_job_queue);
        
    };

    {
        let client = aws_sdk_iam::Client::new(&conf);
        iam_request!(client, create_user);
    };
    
    Ok(())
}
