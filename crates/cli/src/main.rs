mod dict_cmd;
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
        /// Also print partial results while transcribing.
        #[arg(long)]
        partials: bool,
        /// WAV files to transcribe.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Manage recognition models.
    Model {
        #[command(subcommand)]
        cmd: ModelCmd,
    },
    /// Edit the user dictionary.
    Dict {
        #[command(subcommand)]
        cmd: DictCmd,
    },
    /// Print the man page in roff format.
    #[command(hide = true)]
    Manpage,
}

#[derive(Subcommand)]
enum DictCmd {
    /// Add an entry, or replace the entry for the same wrong form.
    Add {
        /// Text as the recognizer writes it.
        wrong: String,
        /// Text to put in its place.
        right: String,
    },
    /// Remove the entry for a wrong form.
    Remove {
        /// Wrong form of the entry.
        wrong: String,
    },
    /// Print every entry as wrong<TAB>right.
    List,
}

#[derive(Subcommand)]
enum ModelCmd {
    /// List the catalog and which models are installed.
    List,
    /// Download and install a model.
    Install {
        /// Model id from the catalog.
        id: String,
        /// Install from a local archive instead of downloading.
        #[arg(long)]
        from_file: Option<PathBuf>,
    },
    /// Remove an installed model.
    Remove {
        /// Model id.
        id: String,
    },
    /// Make an installed model the default, or with --final the final-stage model.
    Use {
        /// Model id; with --final, `none` turns the final-stage model off.
        id: String,
        /// Set the model that decodes the final text; partial text keeps using the default model.
        #[arg(long = "final")]
        final_stage: bool,
    },
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let cli = Cli::parse();
    let paths = koeinput_core::config::Paths::from_env();
    let result = match cli.cmd {
        Cmd::Listen => listen::run(),
        Cmd::Status => listen::status(&paths),
        Cmd::Transcribe { partials, files } => transcribe::run(&paths, partials, &files),
        Cmd::Dict { cmd } => match cmd {
            DictCmd::Add { wrong, right } => dict_cmd::add(&paths, &wrong, &right),
            DictCmd::Remove { wrong } => dict_cmd::remove(&paths, &wrong),
            DictCmd::List => dict_cmd::list(&paths),
        },
        Cmd::Manpage => manpage().map_err(Into::into),
        Cmd::Model { cmd } => match cmd {
            ModelCmd::List => model_cmd::list(&paths),
            ModelCmd::Install { id, from_file } => {
                model_cmd::install(&paths, &id, from_file.as_deref())
            }
            ModelCmd::Remove { id } => model_cmd::remove(&paths, &id),
            ModelCmd::Use { id, final_stage } => {
                if final_stage {
                    model_cmd::use_final(&paths, &id)
                } else {
                    model_cmd::use_model(&paths, &id)
                }
            }
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

const COMMANDS_ROFF: &str = r#".SH COMMANDS
.TP
\fBlisten\fR
Dictate through the daemon: Enter starts, Enter stops, Ctrl\-C cancels.
.TP
\fBstatus\fR
Show the daemon's status.
.TP
\fBtranscribe\fR [\fB\-\-partials\fR] \fIFILE\fR...
Transcribe WAV files in\-process, without the daemon. \fB\-\-partials\fR also prints partial results.
.TP
\fBmodel list\fR
List the catalog and which models are installed.
.TP
\fBmodel install\fR \fIID\fR [\fB\-\-from\-file\fR \fIPATH\fR]
Download and install a model. With \fB\-\-from\-file\fR, install from a local archive instead.
.TP
\fBmodel remove\fR \fIID\fR
Remove an installed model.
.TP
\fBmodel use\fR \fIID\fR
Make an installed model the default.
.TP
\fBmodel use\fR \fIID\fR \fB\-\-final\fR
Use a second model for the final text, while the default model keeps producing the partial text. \fBmodel use \-\-final none\fR turns it off.
.TP
\fBdict add\fR \fIWRONG\fR \fIRIGHT\fR
Add a dictionary entry, or replace the entry for the same \fIWRONG\fR. The daemon picks it up at the next session. Both forms must be non\-empty, so an entry that deletes a word is made by editing the file by hand.
.TP
\fBdict remove\fR \fIWRONG\fR
Remove every entry for \fIWRONG\fR.
.TP
\fBdict list\fR
Print the dictionary entries as \fIWRONG\fR, a tab, and \fIRIGHT\fR.
.SH DICTIONARY FILE
The dictionary is a UTF\-8 text file with one entry per line: the text as the recognizer writes it (\fIWRONG\fR), a tab, and the text to put in its place (\fIRIGHT\fR). Empty lines and lines starting with # are ignored. A line without a tab is skipped with a warning. When \fIRIGHT\fR is empty, the word is deleted from the result; this can only be written by hand. The longest match wins. \fBdict add\fR and \fBdict remove\fR keep comments and the order of lines. The daemon reads the file again at the next session after it changed; a file larger than 1 MiB is not reloaded.
.SH NOTES
Recognition runs locally; no audio leaves the machine. The daemon is started on demand through D\-Bus.
.SH FILES
.TP
\fI~/.config/koeinput/config.toml\fR
Settings, including the selected model.
.TP
\fI~/.config/koeinput/dictionary.tsv\fR
User dictionary.
.TP
\fI~/.local/share/koeinput/models/\fR
Installed models.
.TP
\fI/usr/share/koeinput/models.toml\fR
Model catalog.
"#;

/// The package ships this one page only, so it describes every subcommand itself.
fn manpage() -> std::io::Result<()> {
    use std::io::Write;
    let cmd = Cli::command().disable_help_subcommand(true);
    let man = clap_mangen::Man::new(cmd).date("2026-10");
    let mut out = std::io::stdout().lock();
    man.render_title(&mut out)?;
    man.render_name_section(&mut out)?;
    man.render_synopsis_section(&mut out)?;
    man.render_description_section(&mut out)?;
    man.render_options_section(&mut out)?;
    out.write_all(COMMANDS_ROFF.as_bytes())?;
    man.render_version_section(&mut out)?;
    Ok(())
}
