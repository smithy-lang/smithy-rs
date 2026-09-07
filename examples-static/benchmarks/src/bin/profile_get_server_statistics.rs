use std::env;
use std::process::ExitCode;
use std::time::Instant;

const DEFAULT_ITERATIONS: u64 = 100_000;

fn print_usage(program: &str) {
    eprintln!("usage: {program} <case-name>");
    eprintln!();
    eprintln!("case names:");
    for name in pokemon_service_benchmarks::case_names() {
        eprintln!("  {name}");
    }
    eprintln!();
    eprintln!("set POKEMON_BENCH_ITERS to override the default {DEFAULT_ITERATIONS} iterations");
}

fn iterations() -> u64 {
    env::var("POKEMON_BENCH_ITERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_ITERATIONS)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args = env::args();
    let program = args
        .next()
        .unwrap_or_else(|| "profile_get_server_statistics".to_owned());
    let Some(case_name) = args.next() else {
        print_usage(&program);
        return ExitCode::FAILURE;
    };
    if args.next().is_some() {
        print_usage(&program);
        return ExitCode::FAILURE;
    }

    let Some(case) = pokemon_service_benchmarks::case_by_name(&case_name) else {
        eprintln!("unknown case: {case_name}");
        print_usage(&program);
        return ExitCode::FAILURE;
    };

    let runner = case.runner();
    runner.run().await;

    let iterations = iterations();
    let start = Instant::now();
    for _ in 0..iterations {
        runner.run().await;
    }
    let elapsed = start.elapsed();

    println!(
        "case={case_name} iterations={iterations} elapsed_ns={} avg_ns={}",
        elapsed.as_nanos(),
        elapsed.as_nanos() / u128::from(iterations.max(1)),
    );
    ExitCode::SUCCESS
}
