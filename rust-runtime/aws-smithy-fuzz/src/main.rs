/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(not(windows))]

use aws_smithy_fuzz::{FuzzResult, FuzzTarget, HttpRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::io::{BufRead, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;
use std::{env, fs};

use clap::{Parser, Subcommand};
use tera::{Context, Tera};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Invoke the fuzz driver
    Fuzz(FuzzArgs),

    /// Initialize the fuzz configuration
    ///
    /// This command will compile cdylib shared libraries and create a
    Initialize(InitializeArgs),

    /// Replay subcommand
    Replay(ReplayArgs),

    /// Setup smithy-rs targets
    ///
    /// This does all of the schlep of setting of smithy-rs copies at different revisions for fuzz testing
    /// This command is not strictly necessary—It's pretty easy to wire this up yourself. But
    /// this removes a lot of boilerplate.
    SetupSmithy(SetupSmithyArgs),

    /// Invoke Testcase
    ///
    /// Directly invoke a test case against a given shared library target. Generally, replay will
    /// be easier to use.
    ///
    /// This exists to facilitate invoking a cdylib that may panic without crashing the main
    /// process
    InvokeTestCase(InvokeArgs),
}

#[derive(Parser)]
struct InvokeArgs {
    #[arg(short, long)]
    shared_library_path: PathBuf,

    #[arg(short, long)]
    test_case: PathBuf,
}

#[derive(Parser)]
struct SetupSmithyArgs {
    #[arg(short, long)]
    revision: Vec<String>,
    #[arg(short, long)]
    service: String,
    #[arg(short, long)]
    workdir: Option<PathBuf>,

    #[arg(short, long)]
    fuzz_runner_local_path: PathBuf,

    /// Rebuild the local clones of smithy-rs
    #[arg(long)]
    rebuild_local_targets: bool,

    /// Additional dependencies to use. Usually, these provide the model.
    ///
    /// Dependencies should be in the following format: `software.amazon.smithy:smithy-aws-protocol-tests:1.50.0`
    #[arg(long)]
    dependency: Vec<String>,
}

#[derive(Parser)]
struct FuzzArgs {
    /// Custom path to the configuration file.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "smithy-fuzz-config.json"
    )]
    config_path: String,

    #[arg(long)]
    enter_fuzzing_loop: bool,

    /// The number of parallel fuzzers to run
    #[arg(short, long)]
    num_fuzzers: Option<usize>,
}

#[derive(Parser)]
struct InitializeArgs {
    /// Path to the target crates to fuzz (repeat for multiple crates)
    #[arg(short, long, value_name = "PATH")]
    target_crate: Vec<PathBuf>,

    /// Path to the `lexicon.json` defining the dictionary and corpus
    ///
    /// This file is typically produced by the fuzzgen smithy plugin.
    #[arg(short, long, value_name = "PATH")]
    lexicon: PathBuf,

    /// Custom path to the configuration file.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "smithy-fuzz-config.json"
    )]
    config_path: String,

    /// Force a rebuild of target artifacts.
    ///
    /// **NOTE**: You must use this if you change the target artifacts
    #[arg(short, long)]
    force_rebuild: bool,

    /// Compile the target artifacts in release mode
    #[arg(short, long)]
    release: bool,
}

/// Replay crashes from a fuzz run
///
/// By default, this will automatically discover all crashes during fuzzing sessions and rerun them.
/// To rerun a single crash, use `--invoke-only`.
#[derive(Parser)]
struct ReplayArgs {
    /// Custom path to the configuration file.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "smithy-fuzz-config.json"
    )]
    config_path: String,

    /// Invoke only the specified path.
    #[arg(short, long)]
    invoke_only: Option<String>,

    /// Output results in JSON
    #[arg(short, long)]
    json: bool,

    /// Run against the input corpus instead of running against crashes
    ///
    /// This is helpful for sanity checking that everything is working properly
    #[arg(long)]
    corpus: bool,
}

#[derive(Deserialize)]
struct Lexicon {
    corpus: Vec<HttpRequest>,
    dictionary: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FuzzConfig {
    seed: PathBuf,
    targets: Vec<Target>,
    afl_input_dir: PathBuf,
    afl_output_dir: PathBuf,
    dictionaries: Vec<PathBuf>,
    lexicon: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
struct Target {
    package_name: String,
    source: PathBuf,
    shared_library: Option<PathBuf>,
}

impl Target {
    fn human_name(&self) -> String {
        determine_package_name(&self.source.join("Cargo.toml"))
    }
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Fuzz(args) => fuzz(args),
        Commands::Initialize(args) => initialize(args),
        Commands::Replay(args) => replay(args),
        Commands::SetupSmithy(args) => setup_smithy(args),
        Commands::InvokeTestCase(InvokeArgs {
            test_case,
            shared_library_path,
        }) => invoke_testcase(test_case, shared_library_path),
    }
}

fn setup_smithy(args: SetupSmithyArgs) {
    let current_dir = env::current_dir().unwrap();
    let workdir = match &args.workdir {
        Some(relative_workdir) => current_dir.join(relative_workdir),
        None => current_dir.clone(),
    };
    let maven_locals = workdir.join("maven-locals");
    fs::create_dir_all(&maven_locals).unwrap();

    let fuzz_driver_path = maven_locals.join("fuzz-driver");
    let local_path = current_dir.join(&args.fuzz_runner_local_path);
    build_revision(&fuzz_driver_path, &local_path);

    let rust_runtime_for_fuzzer = local_path.join("rust-runtime");

    let fuzzgen_smithy_build =
        generate_smithy_build_json_for_fuzzer(&args, &fuzz_driver_path, &rust_runtime_for_fuzzer);
    let fuzzgen_smithy_build_path = workdir.join("smithy-build-fuzzgen.json");
    fs::write(&fuzzgen_smithy_build_path, &fuzzgen_smithy_build).unwrap();

    for revision in &args.revision {
        let revision_dir = clone_smithyrs(&workdir, &revision, args.rebuild_local_targets);

        let maven_local_subpath = maven_locals.join(&revision);
        build_revision(&maven_local_subpath, &revision_dir);
        let fuzzgen_smithy_build =
            generate_smithy_build_for_target(&maven_local_subpath, &args, &revision, &revision_dir);
        let smithy_build_path = workdir.join(format!("smithy-build-{revision}.json"));
        fs::write(&smithy_build_path, assert_valid_json(&fuzzgen_smithy_build)).unwrap();
        smithy_build(&workdir, &smithy_build_path);
    }
    println!("running smithy build for fuzz harness");
    smithy_build(&workdir, &fuzzgen_smithy_build_path);
}

fn generate_smithy_build_for_target(
    maven_local_subpath: &Path,
    args: &SetupSmithyArgs,
    revision: &str,
    revision_dir: &Path,
) -> String {
    let mut context = Context::new();
    context.insert("maven_local", &maven_local_subpath);
    context.insert("service", &args.service);
    context.insert("revision", revision);
    context.insert("rust_runtime", &revision_dir.join("rust-runtime"));
    context.insert("dependencies", &args.dependency);

    let fuzzgen_smithy_build = Tera::one_off(
        include_str!("../templates/smithy-build-targetcrate.jinja2"),
        &context,
        false,
    )
    .unwrap();
    assert_valid_json(fuzzgen_smithy_build)
}

fn generate_smithy_build_json_for_fuzzer(
    args: &SetupSmithyArgs,
    fuzz_driver_path: &Path,
    rust_runtime_for_fuzzer: &Path,
) -> String {
    let mut context = Context::new();
    context.insert("service", &args.service);
    context.insert("revisions", &args.revision);
    context.insert("maven_local", &fuzz_driver_path);
    context.insert("rust_runtime", &rust_runtime_for_fuzzer);
    context.insert("dependencies", &args.dependency);

    let fuzzgen_smithy_build = Tera::one_off(
        include_str!("../templates/smithy-build-fuzzer.jinja2"),
        &context,
        false,
    )
    .unwrap();
    assert_valid_json(fuzzgen_smithy_build)
}

/// Run smithy build for a given file
///
/// # Arguments
///
/// * `workdir`: Path to the working directory (this is where the `build` directory) will be created / used
/// * `smithy_build_json`: Path to the smithy-build.json file
fn smithy_build(workdir: impl AsRef<Path>, smithy_build_json: impl AsRef<Path>) {
    println!(
        "running smithy build for {}",
        smithy_build_json.as_ref().display()
    );
    // Need to delete classpath.json if it exists to work around small bug in smithy CLI:
    // https://github.com/smithy-lang/smithy/issues/2376
    let _ = fs::remove_file(workdir.as_ref().join("build/smithy").join("classpath.json"));

    let home_dir = homedir::my_home().unwrap().unwrap();
    exec(
        Command::new("rm")
            .arg("-r")
            .arg(format!("{}/.m2", home_dir.display()))
            .current_dir(&workdir),
    );
    exec(
        Command::new("smithy")
            .arg("build")
            .arg("-c")
            .arg(smithy_build_json.as_ref())
            .current_dir(workdir),
    );
}

/// Creates a copy of the smithy-rs repository at a specific revision.
///
/// - If it does not already exist, it will create a local clone of the entire repo.
/// - After that, it will make a local clone of that repo to facilitate checking out a specific
/// revision
///
/// # Arguments
///
/// * `workdir`: Working directory to clone into
/// * `revision`: Revision to check out
/// * `maven_local`: Path to a revisione-specific maven-local directory to build into
///
/// returns: Path to the cloned directory
fn clone_smithyrs(workdir: impl AsRef<Path>, revision: &str, recreate: bool) -> PathBuf {
    let smithy_dir = workdir.as_ref().join("smithy-rs-src");
    if !smithy_dir.exists() {
        exec(
            Command::new("git")
                .args(["clone", "https://github.com/smithy-lang/smithy-rs.git"])
                .arg(&smithy_dir)
                .current_dir(&workdir),
        );
    }
    let copies_dir = workdir.as_ref().join("smithy-rs-copies");
    fs::create_dir_all(&copies_dir).unwrap();

    let revision_dir = copies_dir.join(revision);
    if revision_dir.exists() && !recreate {
        return revision_dir;
    }
    exec(
        Command::new("rm")
            .arg("-rf")
            .arg(&revision_dir)
            .current_dir(&workdir),
    );

    exec(
        Command::new("git")
            .arg("clone")
            .arg(smithy_dir)
            .arg(&revision_dir)
            .current_dir(&workdir),
    );

    exec(
        Command::new("git")
            .args(["checkout", &revision])
            .current_dir(&revision_dir),
    );
    revision_dir
}

fn exec(command: &mut Command) {
    match command.get_current_dir() {
        None => panic!("BUG: all commands should set a working directory"),
        Some(dir) if !dir.is_absolute() => panic!("bug: absolute directory should be set"),
        _ => {}
    };
    let status = match command.spawn().unwrap().wait() {
        Ok(status) => status,
        Err(e) => {
            panic!("{:?} failed: {}", command, e)
        }
    };
    if !status.success() {
        panic!("command failed: {:?}", command);
    }
}

/// Runs `./gradlew publishToMavenLocal` on a given smithy-rs directory
///
/// # Arguments
///
/// * `maven_local`: mavenLocal directory to publish into
/// * `smithy_dir`: smithy-rs source directory to use
fn build_revision(maven_local: impl AsRef<Path>, smithy_dir: impl AsRef<Path>) {
    tracing::info!("building revision from {}", smithy_dir.as_ref().display());
    exec(
        Command::new("./gradlew")
            .args([
                "publishToMavenLocal",
                &format!("-Dmaven.repo.local={}", maven_local.as_ref().display()),
            ])
            .current_dir(&smithy_dir),
    )
}

fn assert_valid_json<T: AsRef<str>>(data: T) -> T {
    match serde_json::from_str::<Value>(data.as_ref()) {
        Err(e) => panic!(
            "failed to generate valid JSON. this is a bug. {}\n\n{}",
            e,
            data.as_ref()
        ),
        Ok(_) => {}
    };
    data
}

fn force_load_libraries(libraries: &[Target]) -> Vec<FuzzTarget> {
    libraries
        .iter()
        .map(|t| {
            t.shared_library
                .as_ref()
                .expect("shared library must be built! run `aws-smithy-fuzz initialize`")
        })
        .map(FuzzTarget::from_path)
        .collect::<Vec<_>>()
}

/// Starts a fuzz session
///
/// This function is a little bit of an snake eating its tail. When it is initially run,
/// it ensures everything is set up properly, then it invokes AFL, passing through
/// all the relevant flags. AFL is actually going to come right back in here—(but with `enter_fuzzing_loop`)
/// set to true. In that case, we just prepare to start actually fuzzing the targets.
fn fuzz(args: FuzzArgs) {
    let config = fs::read_to_string(&args.config_path).unwrap();
    let config: FuzzConfig = serde_json::from_str(&config).unwrap();
    if args.enter_fuzzing_loop {
        let libraries = force_load_libraries(&config.targets);
        enter_fuzz_loop(libraries, None)
    } else {
        eprintln!(
            "Preparing to start fuzzing... {} targets.",
            config.targets.len()
        );
        let base_command = || {
            let mut cmd = Command::new("cargo");
            cmd.args(["afl", "fuzz"])
                .arg("-i")
                .arg(&config.lexicon)
                .arg("-o")
                .arg(&config.afl_output_dir);

            for dict in &config.dictionaries {
                cmd.arg("-x").arg(dict);
            }
            cmd
        };

        let apply_target = |mut cmd: Command| {
            let current_binary =
                std::env::current_exe().expect("could not determine current target");
            cmd.arg(current_binary)
                .arg("fuzz")
                .arg("--config-path")
                .arg(&args.config_path)
                .arg("--enter-fuzzing-loop");
            cmd
        };
        let mut main_runner = base_command();
        main_runner.arg("-M").arg("fuzzer0");

        eprintln!(
            "Switching to AFL with the following command:\n{:?}",
            main_runner
        );
        let mut main = apply_target(main_runner).spawn().unwrap();
        let mut children = vec![];
        for idx in 1..args.num_fuzzers.unwrap_or_default() {
            let mut runner = base_command();
            runner.arg("-S").arg(format!("fuzzer{}", idx));
            runner.stderr(Stdio::null()).stdout(Stdio::null());
            children.push(KillOnDrop(apply_target(runner).spawn().unwrap()));
        }
        main.wait().unwrap();
    }
}
struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.0.kill().unwrap();
    }
}

fn yellow(text: impl Display) {
    let mut stdout = StandardStream::stderr(ColorChoice::Auto);
    use std::io::Write;
    stdout
        .set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))
        .unwrap();
    writeln!(&mut stdout, "{}", text).unwrap();
    stdout.reset().unwrap();
}

fn initialize(
    InitializeArgs {
        target_crate,
        lexicon,
        config_path,
        force_rebuild,
        release,
    }: InitializeArgs,
) {
    let mode = match release {
        true => Mode::Release,
        false => Mode::Debug,
    };
    let current_config = if Path::new(&config_path).exists() {
        let config_data = fs::read_to_string(&config_path).unwrap();
        let config: FuzzConfig = serde_json::from_str(&config_data).unwrap();
        Some(config)
    } else {
        None
    };
    let current_targets = current_config.map(|c| c.targets).unwrap_or_default();
    let targets = if current_targets
        .iter()
        .map(|t| &t.source)
        .collect::<HashSet<_>>()
        != target_crate.iter().collect::<HashSet<_>>()
    {
        yellow("The target crates specified in the configuration file do not match the current target crates.");
        eprintln!(
            "Initializing the fuzzer with {} target crates.",
            target_crate.len()
        );
        target_crate
            .into_iter()
            .map(initialize_target)
            .collect::<Vec<_>>()
    } else {
        current_targets
    };
    if targets.is_empty() {
        yellow("No target crates specified, nothing to do.");
    }

    let afl_input_dir = Path::new("afl-input");
    let afl_output_dir = Path::new("afl-output");

    let mut config = FuzzConfig {
        seed: lexicon,
        targets,
        afl_input_dir: afl_input_dir.into(),
        afl_output_dir: afl_output_dir.into(),
        lexicon: afl_input_dir.join("corpus"),
        dictionaries: vec![afl_input_dir.join("dictionary")],
    };

    let seed_request = fs::read_to_string(&config.seed).unwrap();
    let seed: Lexicon = serde_json::from_str(&seed_request).unwrap();
    write_seed(&config.afl_input_dir, &seed);

    for target in &mut config.targets {
        if target.shared_library.is_none() || force_rebuild {
            build_target(target, mode);
        }
        check_library_health(&FuzzTarget::from_path(
            target.shared_library.as_ref().unwrap(),
        ));
    }
    eprintln!("Writing settings to {}", config_path);
    fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
}

fn initialize_target(source: PathBuf) -> Target {
    let package_id = determine_package_id(source.as_ref());
    Target {
        package_name: package_id,
        source,
        shared_library: None,
    }
}

fn load_all_crashes(output_dir: &Path) -> Vec<PathBuf> {
    let pattern = output_dir.join("fuzzer*/crashes*");
    load_inputs_at_pattern(&pattern)
}

fn load_corpus(input_dir: &Path) -> Vec<PathBuf> {
    let pattern = input_dir.join("corpus");
    load_inputs_at_pattern(&pattern)
}

fn load_inputs_at_pattern(pattern: &Path) -> Vec<PathBuf> {
    eprintln!("searching for test cases in {}", pattern.display());
    let pattern = format!("{}", pattern.display());
    let mut crash_directories = glob::glob(&pattern).unwrap();
    let mut crashes = vec![];
    while let Some(Ok(crash_dir)) = crash_directories.next() {
        for entry in fs::read_dir(crash_dir).unwrap() {
            let entry = entry.unwrap().path();
            match entry.file_name().and_then(|f| f.to_str()) {
                None | Some("README.txt") => {}
                Some(_other) => crashes.push(entry),
            }
        }
    }
    crashes
}

/// Replay crashes
fn replay(
    ReplayArgs {
        config_path,
        invoke_only,
        json,
        corpus,
    }: ReplayArgs,
) {
    let config = fs::read_to_string(config_path).unwrap();
    let config: FuzzConfig = serde_json::from_str(&config).unwrap();
    let crashes = if let Some(path) = invoke_only {
        vec![path.into()]
    } else {
        match corpus {
            true => load_corpus(&config.afl_input_dir),
            false => load_all_crashes(&config.afl_output_dir),
        }
    };
    eprintln!("Replaying {} crashes.", crashes.len());
    for crash in crashes {
        eprintln!("{}", crash.display());
        let data = fs::read(&crash).unwrap();
        let http_request = HttpRequest::from_unknown_bytes(&data);
        let mut results: HashMap<String, CrashResult> = HashMap::new();
        #[derive(Debug, Serialize)]
        #[serde(tag = "type")]
        enum CrashResult {
            Panic { message: String },
            FuzzResult { result: String },
        }

        impl Display for CrashResult {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    CrashResult::Panic { message } => {
                        f.pad("The process paniced!\n")?;
                        for line in message.lines() {
                            write!(f, " {}\n", line)?;
                        }
                        Ok(())
                    }
                    CrashResult::FuzzResult { result } => f.pad(result),
                }
            }
        }

        for library in &config.targets {
            let result = Command::new(env::current_exe().unwrap())
                .arg("invoke-test-case")
                .arg("--shared-library-path")
                .arg(library.shared_library.as_deref().unwrap())
                .arg("--test-case")
                .arg(&crash)
                .output()
                .unwrap();
            let result = match serde_json::from_slice::<FuzzResult>(&result.stdout) {
                Ok(result) => CrashResult::FuzzResult {
                    result: format!("{:?}", result),
                },
                Err(_err) => CrashResult::Panic {
                    message: String::from_utf8_lossy(&result.stderr).to_string(),
                },
            };
            results.insert(library.human_name(), result);
        }
        #[derive(Serialize)]
        struct Results {
            test_case: String,
            results: HashMap<String, CrashResult>,
        }
        impl Display for Results {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let test_case = &self.test_case;
                write!(f, "Test case: {test_case}\n")?;
                for (target, result) in &self.results {
                    write!(f, " target: {target}\n{:>2}", result)?;
                }
                Ok(())
            }
        }
        let results = Results {
            test_case: format!("{:#?}", http_request.unwrap()),
            results,
        };
        if json {
            println!("{}", serde_json::to_string(&results).unwrap());
        } else {
            println!("{}\n----", results);
        }
    }
}

fn invoke_testcase(test_case: impl AsRef<Path>, shared_library_path: impl AsRef<Path>) {
    let data = fs::read(test_case).unwrap();
    let library = FuzzTarget::from_path(shared_library_path);
    let data = data.clone();
    let result = library.invoke_bytes(&data.clone());
    println!("{}", serde_json::to_string(&result).unwrap());
}

/// Enters the fuzzing loop. This method should only be entered when `afl` is driving the binary
fn enter_fuzz_loop(libraries: Vec<FuzzTarget>, mut log: Option<BufWriter<fs::File>>) {
    afl::fuzz(true, |data: &[u8]| {
        use std::io::Write;
        #[allow(clippy::disallowed_methods)]
        let start = SystemTime::now();

        let http_request = HttpRequest::from_unknown_bytes(data);
        if let Some(request) = http_request {
            if request.into_http_request_04x().is_some() {
                let mut results = vec![];
                for library in &libraries {
                    results.push(library.invoke_bytes(data));
                }
                log.iter_mut().for_each(|log| {
                    log.write_all(
                        #[allow(clippy::disallowed_methods)]
                        format!(
                            "[{:?}ms] {:?} {:?}\n",
                            start.elapsed().unwrap().as_millis(),
                            request,
                            &results[0]
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                });
                for result in &results {
                    if result.response != results[0].response {
                        if check_for_nondeterminism(data, &libraries) {
                            break;
                        }
                        panic!("inconsistent results: {:#?}", results);
                    }
                }
            }
        }
    });
}

fn check_for_nondeterminism(data: &[u8], libraries: &[FuzzTarget]) -> bool {
    for lib in libraries {
        let sample = (0..10)
            .map(|_idx| lib.invoke_bytes(&data))
            .collect::<Vec<_>>();
        if sample.iter().any(|result| result != &sample[0]) {
            return true;
        }
    }
    return false;
}

/// Converts a JSON formatted seed to the format expected by AFL
///
/// - Dictionary items are written out into a file
/// - Corpus items are bincode serialized so that the format matches
fn write_seed(target_directory: &Path, seed: &Lexicon) {
    fs::create_dir_all(target_directory.join("corpus")).unwrap();
    for (id, request) in seed.corpus.iter().enumerate() {
        std::fs::write(
            target_directory.join("corpus").join(&format!("{}", id)),
            request.as_bytes(),
        )
        .unwrap();
    }
    use std::fmt::Write;
    let mut dictionary = String::new();
    for word in &seed.dictionary {
        writeln!(dictionary, "\"{word}\"").unwrap();
    }
    std::fs::write(target_directory.join("dictionary"), dictionary.as_bytes()).unwrap();
}

fn check_library_health(library: &FuzzTarget) {
    let input = HttpRequest {
        uri: "/NoInputAndNoOutput".to_string(),
        method: "POST".to_string(),
        headers: [("Accept".to_string(), vec!["application/json".to_string()])]
            .into_iter()
            .collect::<HashMap<_, _>>(),
        trailers: Default::default(),
        body: "{}".into(),
    };
    library.invoke(&input);
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
enum Mode {
    Release,
    Debug,
}

impl AsRef<Path> for Mode {
    fn as_ref(&self) -> &Path {
        match self {
            Mode::Release => Path::new("release"),
            Mode::Debug => Path::new("debug"),
        }
    }
}

fn determine_package_name(path: &Path) -> String {
    let cargo_toml_file = cargo_toml::Manifest::from_path(path).expect("invalid manifest");
    cargo_toml_file.package.unwrap().name
}

fn determine_package_id(path: &Path) -> String {
    let metadata = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(path)
        .output()
        .unwrap();
    let metadata: Value = match serde_json::from_slice(&metadata.stdout) {
        Ok(v) => v,
        Err(e) => panic!(
            "failed to parse metadata: {}\n{}",
            e,
            String::from_utf8_lossy(&metadata.stderr)
        ),
    };
    let package_name = &metadata["workspace_members"][0].as_str().unwrap();
    package_name.to_string()
}

fn build_target(target: &mut Target, mode: Mode) {
    let mut cmd = Command::new("cargo");
    cmd.env(
        "CARGO_TARGET_DIR",
        env::current_dir()
            .unwrap()
            .join("target/fuzz-target-target"),
    );
    cmd.args(["afl", "build", "--message-format", "json"]);
    cmd.stdout(Stdio::piped());
    if mode == Mode::Release {
        cmd.arg("--release");
    }
    let json_output = cmd
        .current_dir(&target.source)
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();

    let shared_library = json_output
        .stdout
        .lines()
        .filter_map(|line| cargo_output::find_shared_library(&line.unwrap(), target))
        .next()
        .expect("failed to find shared library");
    target.shared_library = Some(PathBuf::from(shared_library))
}

mod cargo_output {
    use crate::Target;
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct DylibOutput {
        reason: String,
        package_id: String,
        target: DyLibTarget,
        filenames: Vec<String>,
    }

    #[derive(Deserialize, Debug)]
    struct DyLibTarget {
        kind: Vec<String>,
    }

    /// Reads a line of cargo output and look for the dylib
    pub(super) fn find_shared_library(line: &str, target: &Target) -> Option<String> {
        tracing::trace!("{}", line);
        let output: DylibOutput = match serde_json::from_str(line) {
            Ok(output) => output,
            Err(_e) => {
                tracing::debug!("line does not match: {}", line);
                return None;
            }
        };
        if output.reason != "compiler-artifact" {
            return None;
        }
        if output.package_id != target.package_name {
            tracing::debug!(expected = %target.package_name, actual = %output.package_id, "ignoring line—package was wrong");
            return None;
        }
        if output.target.kind != ["cdylib"] {
            return None;
        }
        Some(
            output
                .filenames
                .into_iter()
                .next()
                .expect("should be one dylib target"),
        )
    }
}

#[cfg(test)]
mod test {
    use std::{env::temp_dir, path::PathBuf};

    use crate::{
        generate_smithy_build_for_target, generate_smithy_build_json_for_fuzzer, SetupSmithyArgs,
    };

    #[test]
    fn does_this_work() {
        let path = PathBuf::new();
        let args = SetupSmithyArgs {
            revision: vec!["main".into()],
            service: "test-service".to_string(),
            workdir: Some(temp_dir()),
            fuzz_runner_local_path: "../".into(),
            rebuild_local_targets: true,
            dependency: vec![],
        };
        generate_smithy_build_json_for_fuzzer(&args, &path, &path);
        generate_smithy_build_for_target(&path, &args, "revision", &path);
    }
}
