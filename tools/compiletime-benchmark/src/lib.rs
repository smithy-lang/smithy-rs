pub fn force_dry_run() -> bool {
    option_env!("DRY_RUN") != Some("FALSE")
}


pub fn common_tag<T: DeserializeOwned>() -> T {
    toml::from_str(include_str!("../config/common/tag.toml")).unwrap()
}
