use blitzline_adapters::Store;
use blitzline_domain::*;
use blitzline_engine::{Engine, Repository};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Blitzline interactive prototype — simulated processing")]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    List,
    Example,
    Show { id: u64 },
    Prompt { id: u64 },
    Submit { id: u64, file: PathBuf },
    Continue { id: u64 },
    Pause { id: u64 },
    Advance,
    Preview { id: u64 },
    Export { id: u64, output: PathBuf },
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = Args::parse();
    let store = Store::open(args.data_dir.unwrap_or_else(Store::default_directory))?;
    // One-shot commands must not pause queued work simply by listing it.
    let mut engine = Engine {
        state: store.load()?,
    };
    let mut changed = true;
    match args.command {
        Command::List => {
            println!(
                "{}",
                serde_json::to_string_pretty(&engine.state.projects).unwrap()
            );
            changed = false;
        }
        Command::Example => {
            println!("Created example {}", engine.example()?);
        }
        Command::Show { id } => {
            println!(
                "{}",
                serde_json::to_string_pretty(engine.project(id)?).unwrap()
            );
            changed = false;
        }
        Command::Prompt { id } => {
            println!("{}", engine.prompt(id)?);
            changed = false;
        }
        Command::Submit { id, file } => {
            let raw = std::fs::read_to_string(file).map_err(|e| Problem(e.to_string()))?;
            let result = engine.submit(id, &raw)?;
            println!("Saved {}; remaining {}", result.accepted, result.remaining);
        }
        Command::Continue { id } => {
            if engine
                .project(id)?
                .exchange
                .as_ref()
                .is_some_and(|e| e.accepted.len() == e.requested.len())
            {
                engine.next_exchange(id)?;
            } else {
                engine.resume(id)?;
            }
        }
        Command::Pause { id } => engine.pause(id)?,
        Command::Advance => engine.tick(),
        Command::Preview { id } => engine.preview(id)?,
        Command::Export { id, output } => {
            let text = engine.tsv(id)?;
            std::fs::write(output, text).map_err(|e| Problem(e.to_string()))?;
            engine.export_demo(id, "Sample TSV")?;
        }
    }
    if changed {
        store.save(&engine.state)?;
    }
    Ok(())
}
