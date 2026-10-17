mod listen;
mod model_cmd;
mod transcribe;

use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "koeinput", version, about = "Japanese voice input (KoeInput)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dictate through the daemon: Enter starts, Enter stops, Ctrl-C cancels.
    Listen,
    /// Show the daemon's status.
    Status,
    /// Transcribe WAV files in-process, without the daemon.
    Transcribe {
        #[arg(long)]
        partials: bool,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Manage recognition models.
    Model {
        #[command(subcommand)]
        cmd: ModelCmd,
    },
    /// Print the man page in roff format.
    #[command(hide = true)]
    Manpage,
}

#[derive(Subcommand)]
enum ModelCmd {
    List,
    Install {
        id: String,
        #[arg(long)]
        from_file: Option<PathBuf>,
    },
    Remove {
        id: String,
    },
    Use {
        id: String,
    },
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let cli = Cli::parse();
    let paths = koeinput_core::config::Paths::from_env();
    let result = match cli.cmd {
        Cmd::Listen => listen::run(),
        Cmd::Status => listen::status(),
        Cmd::Transcribe { partials, files } => transcribe::run(&paths, partials, &files),
        Cmd::Manpage => clap_mangen::Man::new(Cli::command())
            .render(&mut std::io::stdout())
            .map_err(Into::into),
        Cmd::Model { cmd } => match cmd {
            ModelCmd::List => model_cmd::list(&paths),
            ModelCmd::Install { id, from_file } => {
                model_cmd::install(&paths, &id, from_file.as_deref())
            }
            ModelCmd::Remove { id } => model_cmd::remove(&paths, &id),
            ModelCmd::Use { id } => model_cmd::use_model(&paths, &id),
        },
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("koeinput: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
