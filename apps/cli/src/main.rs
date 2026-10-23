use blitzline_adapters::pipeline::{self as runtime, Runtime, config};
use blitzline_domain::pipeline::*;
use blitzline_engine::pipeline::{Backend, Pipeline};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
#[derive(Parser)]
#[command(
    name = "blitzline",
    version,
    about = "Turn recordings and text into reviewed study cards. Native Rust; no Python runtime."
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Start a saved text or recording pipeline.
    Run(Run),
    /// Continue a saved Rust run. May make configured API requests.
    Resume(Resume),
    /// Export validated cards without repeating processing.
    Export {
        run_dir: PathBuf,
        #[arg(long,value_parser=["tsv","apkg","anki"])]
        format: String,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        deck_profile: Option<String>,
        #[arg(long, conflicts_with = "no_allow_partial")]
        allow_partial: bool,
        #[arg(long)]
        no_allow_partial: bool,
    },
    /// Print the current browser-chat request without sending it.
    Prompt { run_dir: PathBuf },
    /// Validate a pasted response. Does not dispatch the next paid request.
    Submit {
        run_dir: PathBuf,
        #[arg(long)]
        file: PathBuf,
    },
    /// Inspect a saved run without starting it.
    Show { run_dir: PathBuf },
    /// List saved runs without starting them.
    List,
    /// Request a graceful pause of a running process.
    Pause { run_dir: PathBuf },
    /// Mark a failed or interrupted stage for retry, without dispatching it.
    RetryStage { run_dir: PathBuf, stage: String },
    /// Preview cleanup of a completed portable export. --apply removes intermediates.
    Clean {
        run_dir: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    /// Verify native tools and configuration without paid requests.
    Check {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, short = 'l')]
        language: Option<String>,
        #[arg(long)]
        text_only: bool,
    },
    /// Describe this binary's supported contracts.
    Capabilities,
    /// Install or inspect checksum-pinned native tools and models.
    Resources {
        #[command(subcommand)]
        command: Resources,
    },
}
#[derive(Subcommand)]
enum Resources {
    List,
    Install {
        manifest: PathBuf,
        #[arg(long)]
        name: String,
    },
}
#[derive(Args, Default)]
struct Options {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    deck_profile: Option<String>,
    #[arg(long,value_parser=["tsv","apkg","anki"])]
    format: Option<String>,
    #[arg(long,value_parser=["audio","video"])]
    clips: Option<String>,
    #[arg(long, conflicts_with = "no_proofread")]
    proofread: bool,
    #[arg(long)]
    no_proofread: bool,
    #[arg(long, conflicts_with = "no_translate")]
    translate: bool,
    #[arg(long)]
    no_translate: bool,
    #[arg(long, conflicts_with = "no_allow_partial")]
    allow_partial: bool,
    #[arg(long)]
    no_allow_partial: bool,
    #[arg(long,value_parser=["auto","browser","manual","api"])]
    method: Option<String>,
    #[arg(long,value_parser=["local","http","groq"])]
    asr_backend: Option<String>,
    #[arg(long, conflicts_with = "asr_json")]
    srt: Option<PathBuf>,
    #[arg(long)]
    asr_json: Option<PathBuf>,
    /// Import the documented Blitzer JSON contract, bypassing extraction.
    #[arg(long)]
    vocabulary_json: Option<PathBuf>,
    #[arg(long)]
    review: Option<PathBuf>,
    #[arg(long,value_parser=STAGES)]
    stop_after: Option<String>,
}
#[derive(Args)]
struct Run {
    source: PathBuf,
    #[arg(long)]
    text_only: bool,
    #[arg(long, short = 'l')]
    language: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[command(flatten)]
    options: Options,
}
#[derive(Args)]
struct Resume {
    run_dir: PathBuf,
    #[arg(long)]
    text_file: Option<PathBuf>,
    #[command(flatten)]
    options: Options,
}
fn apply(s: &mut Value, o: &Options) -> Result<()> {
    for (section, key, v) in [
        ("export", "format", o.format.as_ref()),
        ("export", "deck_profile", o.deck_profile.as_ref()),
        ("media", "kind", o.clips.as_ref()),
        ("asr", "backend", o.asr_backend.as_ref()),
        ("run", "method", o.method.as_ref()),
    ] {
        if let Some(v) = v {
            s[section][key] = json!(v);
        }
    }
    if let Some(method) = &o.method {
        for key in ["proofread_method", "translate_method", "review_method"] {
            s["run"][key] = json!(method);
        }
    }
    for (key, yes, no) in [
        ("proofread", o.proofread, o.no_proofread),
        ("translate", o.translate, o.no_translate),
        ("allow_partial", o.allow_partial, o.no_allow_partial),
    ] {
        if yes || no {
            s["run"][key] = json!(yes);
        }
    }
    config::validate(s)
}
fn request_options(
    p: &mut Pipeline<Runtime>,
    o: &Options,
    text_file: Option<PathBuf>,
) -> Result<()> {
    if p.job.request.source_kind == "text"
        && (o.srt.is_some() || o.asr_json.is_some() || o.clips.is_some() || o.asr_backend.is_some())
    {
        return Err(Error::input(
            "--text-only cannot be combined with --srt, --asr-json, --clips, or --asr-backend",
        ));
    }
    if text_file.is_some() && p.job.request.source_kind != "text" {
        return Err(Error::input("--text-file requires a text-only run"));
    }
    let absolute = |path: &Path| {
        path.canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| Error::input(format!("Cannot read input {}: {e}", path.display())))
    };
    if let Some(path) = &o.srt {
        p.job.request.srt = Some(absolute(path)?);
        p.job.request.asr_json = None;
    }
    if let Some(path) = &o.asr_json {
        p.job.request.asr_json = Some(absolute(path)?);
        p.job.request.srt = None;
    }
    if let Some(path) = &o.vocabulary_json {
        p.job.request.vocabulary_json = Some(absolute(path)?);
    }
    if let Some(path) = text_file {
        p.job.request.text_file = Some(absolute(&path)?);
    }
    p.backend.save(&p.job)?;
    if let Some(path) = &o.review {
        p.import_review(&std::fs::read_to_string(path)?)?;
    }
    Ok(())
}
fn read_job(root: &Path) -> Result<Job> {
    let j: Job = serde_json::from_slice(&std::fs::read(root.join("manifest.json"))?)?;
    if j.implementation != "blitzline-rust" || j.schema_version != 1 {
        return Err(Error::input(
            "Unsupported run; existing Python runs remain with blitzline-py",
        ));
    }
    Ok(j)
}
fn result(value: &Value, json_output: bool) {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else if let Some(dir) = value["run"].as_str() {
        println!("Run: {dir}");
        println!(
            "State: {}; cards: {}; omitted: {}",
            value["state"].as_str().unwrap_or("unknown"),
            value["cards"].as_u64().unwrap_or(0),
            value["omitted"].as_u64().unwrap_or(0)
        );
        if let Some(message) = value["message"].as_str().filter(|m| !m.is_empty()) {
            println!("{message}");
        }
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}
fn snapshot(p: &Pipeline<Runtime>) -> Value {
    json!({"run":p.backend.root,"state":p.job.state,"cards":p.job.cards,"omitted":p.job.omitted,"message":p.job.message,"pending":p.job.pending.as_ref().map(|t|&t.id),"exports":p.job.exports})
}
fn main() {
    let cli = Cli::parse();
    let json_output = cli.json;
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    if let Err(e) =
        ctrlc::set_handler(move || flag.store(true, std::sync::atomic::Ordering::Relaxed))
    {
        eprintln!("Could not install interrupt handler: {e}");
    }
    if let Err(e) = run(cli, cancel) {
        if json_output {
            eprintln!("{}", json!({"error":{"code":e.code,"message":e.message}}));
        } else {
            eprintln!("{}", e.message);
        }
        std::process::exit(e.exit_code());
    }
}
fn run(cli: Cli, cancel: Arc<AtomicBool>) -> Result<()> {
    let json_output = cli.json;
    match cli.command {
        Command::Run(args) => {
            if args.text_only
                && (args.options.srt.is_some()
                    || args.options.asr_json.is_some()
                    || args.options.clips.is_some()
                    || args.options.asr_backend.is_some())
            {
                return Err(Error::input(
                    "--text-only cannot be combined with --srt, --asr-json, --clips, or --asr-backend",
                ));
            }
            let mut settings = config::load(args.options.config.as_deref())?;
            apply(&mut settings, &args.options)?;
            let language = args
                .language
                .or_else(|| {
                    let name = settings["export"]["deck_profile"].as_str()?;
                    settings["decks"][name]["language"]
                        .as_str()
                        .map(str::to_owned)
                })
                .ok_or_else(|| Error::input("Specify --language or a configured --deck-profile"))?;
            let root = args.output.unwrap_or_else(|| {
                cli.data_dir
                    .unwrap_or_else(|| {
                        PathBuf::from(settings["run"]["output_dir"].as_str().unwrap())
                    })
                    .join(format!(
                        "{}-{}",
                        runtime::now(),
                        args.source
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                    ))
            });
            let mut pipeline = runtime::start(
                &root,
                &args.source,
                &language,
                if args.text_only { "text" } else { "media" },
                settings,
                cancel,
            )?;
            pipeline.backend.verbose = !json_output;
            request_options(&mut pipeline, &args.options, None)?;
            let status = pipeline.process(args.options.stop_after.as_deref());
            result(&snapshot(&pipeline), json_output);
            status?;
        }
        Command::Resume(args) => {
            let mut p = runtime::open(&args.run_dir, cancel)?;
            p.backend.verbose = !json_output;
            if let Some(path) = &args.options.config {
                p.job.settings = config::load(Some(path))?;
            }
            apply(&mut p.job.settings, &args.options)?;
            let pause = p.backend.path(".pause")?;
            if pause.exists() {
                std::fs::remove_file(pause)?;
            }
            request_options(&mut p, &args.options, args.text_file)?;
            let status = p.process(args.options.stop_after.as_deref());
            result(&snapshot(&p), json_output);
            status?;
        }
        Command::Export {
            run_dir,
            format,
            config: path,
            deck_profile,
            allow_partial,
            no_allow_partial,
        } => {
            let mut p = runtime::open(&run_dir, cancel)?;
            if let Some(path) = path {
                let new = config::load(Some(&path))?;
                p.job.settings["export"] = new["export"].clone();
                p.job.settings["decks"] = new["decks"].clone();
            }
            p.job.settings["export"]["format"] = json!(format);
            if let Some(name) = deck_profile {
                p.job.settings["export"]["deck_profile"] = json!(name);
            }
            if allow_partial || no_allow_partial {
                p.job.settings["run"]["allow_partial"] = json!(allow_partial);
            }
            let paths = p.export_current()?;
            result(
                &json!({"paths":paths.iter().map(|s|p.backend.root.join(s)).collect::<Vec<_>>()}),
                json_output,
            );
        }
        Command::Prompt { run_dir } => {
            let job = read_job(&run_dir)?;
            let task = job
                .pending
                .ok_or_else(|| Error::input("This run has no pending exchange"))?;
            if json_output {
                result(
                    &json!({"exchange_id":task.id,"revision":task.revision,"prompt":task.prompt()}),
                    true,
                );
            } else {
                println!("{}", task.prompt());
            }
        }
        Command::Submit { run_dir, file } => {
            let mut p = runtime::open(&run_dir, cancel)?;
            let mut raw = String::new();
            if file == Path::new("-") {
                std::io::stdin()
                    .take(4 * 1024 * 1024 + 1)
                    .read_to_string(&mut raw)?;
            } else {
                std::fs::File::open(file)?
                    .take(4 * 1024 * 1024 + 1)
                    .read_to_string(&mut raw)?;
            }
            if raw.len() > 4 * 1024 * 1024 {
                return Err(Error::input("Reply exceeds 4 MiB"));
            }
            p.submit(&raw)?;
            result(
                &json!({"saved":true,"accepted":p.job.pending.as_ref().map(|t|t.accepted.len()),"message":"Reply saved. Resume the run to continue."}),
                json_output,
            );
        }
        Command::Show { run_dir } => result(&serde_json::to_value(read_job(&run_dir)?)?, true),
        Command::List => {
            let root = cli
                .data_dir
                .unwrap_or_else(|| config::data_directory().join("runs"));
            let mut jobs = Vec::new();
            if root.is_dir() {
                for entry in std::fs::read_dir(root)? {
                    let path = entry?.path();
                    if path.is_dir() && path.join("manifest.json").is_file() {
                        match read_job(&path){Ok(j)=>jobs.push(json!({"run":path,"state":j.state,"cards":j.cards,"source":j.request.source})),Err(e)=>jobs.push(json!({"run":path,"error":e.message}))}
                    }
                }
            }
            result(&json!(jobs), true);
        }
        Command::Pause { run_dir } => {
            let job = read_job(&run_dir)?;
            if job.state == "cleaned" || job.state == "complete" {
                return Err(Error::input("This run has no active processing to pause"));
            }
            let root = run_dir.canonicalize()?;
            runtime::atomic(&root.join(".pause"), b"pause\n")?;
            result(
                &json!({"state":"pause-requested","message":"No new work will start; in-flight HTTP responses are saved before pausing."}),
                json_output,
            );
        }
        Command::RetryStage { run_dir, stage } => {
            let mut p = runtime::open(&run_dir, cancel)?;
            let stage = if stage.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                stage
            } else {
                match stage.as_str() {
                    "transcript" => "01-transcript",
                    "proofread" => "02-proofread",
                    "translate" => "03-translate",
                    "vocabulary" => "04-vocabulary",
                    "review" => "05-review",
                    "media" => "06-media",
                    _ => return Err(Error::input("Unknown retry stage")),
                }
                .into()
            };
            let previous = p
                .job
                .stages
                .get(&stage)
                .ok_or_else(|| Error::input("No such stage has run"))?;
            if previous.state == "complete" {
                return Err(Error::input(
                    "Select a failed, interrupted, or pending stage to retry",
                ));
            }
            p.job.stages.retain(|n, _| n < &stage);
            if let Some(task) = &mut p.job.pending {
                task.attempts = 0;
            }
            p.job.state = "paused".into();
            p.backend.save(&p.job)?;
            result(
                &json!({"stage":stage,"state":"retry-requested"}),
                json_output,
            );
        }
        Command::Clean { run_dir, apply } => {
            let p = runtime::open(&run_dir, cancel)?;
            result(&p.backend.cleanup(&p.job, apply)?, true);
        }
        Command::Check {
            config: path,
            language,
            text_only,
        } => {
            let s = config::load(path.as_deref())?;
            let mut tools = serde_json::Map::new();
            let mut missing = Vec::new();
            for (name, section, key) in [
                ("Blitzer", "blitzer", "executable"),
                ("FFmpeg", "media", "ffmpeg"),
                ("FFprobe", "media", "ffprobe"),
                ("whisper.cpp", "asr", "executable"),
            ] {
                if text_only && name != "Blitzer"
                    || name == "whisper.cpp" && s["asr"]["backend"] != "local"
                {
                    tools.insert(name.into(), json!("not required"));
                    continue;
                }
                let executable = s[section][key].as_str().unwrap();
                let checked = runtime::find_tool(executable);
                if let Err(e) = &checked {
                    missing.push(e.message.clone());
                }
                tools.insert(
                    name.into(),
                    json!(
                        checked
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|e| e.message)
                    ),
                );
            }
            if let Some(language) = language {
                config::export_settings(&s, &language)?;
            }
            result(
                &json!({"tools":tools,"python_required":false,"package_writer":"built in","profiles":s["profiles"].as_object().unwrap().iter().map(|(n,p)|json!({"name":n,"model":p["model"],"credential_set":p["api_key_env"].as_str().is_some_and(|n|std::env::var_os(n).is_some())})).collect::<Vec<_>>()}),
                true,
            );
            if !missing.is_empty() {
                return Err(Error::operation(
                    "Some native tools are unavailable. Text runs with --vocabulary-json need no external tools.",
                ));
            }
        }
        Command::Capabilities => result(
            &json!({"implementation":"rust","schema_version":1,"python_required":false,"text_only":true,"formats":["tsv","apkg","anki"],"methods":["api","browser","manual"],"asr":["whisper.cpp","groq","http","srt","asr-json"],"commands":["run","resume","export","check","capabilities","retry-stage","clean","prompt","submit","show","list","pause","resources"],"compiled_blitzer_required_for_extraction":true,"python_job_resume":false}),
            true,
        ),
        Command::Resources { command } => match command {
            Resources::List => {
                let path = config::data_directory().join("resources");
                let mut files = Vec::new();
                if path.is_dir() {
                    for entry in walk_resources(&path)? {
                        files.push(entry);
                    }
                }
                result(
                    &json!({"platform":runtime::resources::platform(),"directory":path,"installed":files}),
                    true,
                );
            }
            Resources::Install { manifest, name } => {
                let catalog: runtime::resources::Catalog =
                    serde_json::from_slice(&std::fs::read(manifest)?)?;
                let rt = Runtime::open(
                    &config::data_directory().join("resources/.operation"),
                    cancel,
                )?;
                let path = runtime::resources::install(&rt, &catalog, &name)?;
                result(&json!({"installed":path}), true);
            }
        },
    }
    Ok(())
}
fn walk_resources(path: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            out.extend(walk_resources(&entry.path())?);
        } else if entry.path().extension().is_some_and(|e| e == "json") {
            out.push(entry.path());
        }
    }
    Ok(out)
}
