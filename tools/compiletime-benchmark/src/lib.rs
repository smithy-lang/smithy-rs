pub fn force_dry_run() -> bool {
    option_env!("DRY_RUN") != Some("FALSE")
}
