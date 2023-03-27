pub fn is_dry_run() -> bool {
    option_env!("IS_DRY_RUN") != Some("FALSE")
}