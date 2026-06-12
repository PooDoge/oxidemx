use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use oxidemx_widget_cli::{install, pack, verify};
use oxidemx_widget_host::SignatureState;

/// OxideMX widget bundle tool — pack, verify, and install `.oxw` widget bundles.
#[derive(Parser)]
#[command(name = "oxidemx-widget", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pack a widget directory into a signed `.oxw` bundle.
    Pack {
        /// Path to the widget directory (must contain widget.json).
        dir: PathBuf,
        /// Output path for the `.oxw` file (default: `<dir_name>.oxw`).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Verify the signature of a `.oxw` bundle and print the signer fingerprint.
    Verify {
        /// Path to the `.oxw` bundle.
        bundle: PathBuf,
    },
    /// Install a `.oxw` bundle into the OxideMX widgets directory.
    Install {
        /// Path to the `.oxw` bundle.
        bundle: PathBuf,
        /// Replace an existing widget with the same id.
        #[arg(long)]
        force: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Pack { dir, output } => match pack(&dir, output.as_deref()) {
            Ok(result) => {
                println!(
                    "Packed {} (signer fingerprint: {})",
                    result.path.display(),
                    result.fingerprint
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },

        Commands::Verify { bundle } => match verify(&bundle) {
            Ok(result) => {
                match &result.state {
                    SignatureState::Pinned => {
                        println!("Signature valid — Pinned (fingerprint: {})", result.fingerprint);
                    }
                    SignatureState::Unknown(fp) => {
                        println!("Signature valid — Unknown signer (fingerprint: {fp})");
                    }
                    SignatureState::Unsigned => {
                        println!("No SIGNATURE entry — bundle is unsigned");
                    }
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },

        Commands::Install { bundle, force } => {
            match install(&bundle, force, None) {
                Ok(result) => {
                    let sig_desc = match &result.signature_state {
                        SignatureState::Pinned => format!("Pinned ({})", result.fingerprint),
                        SignatureState::Unknown(fp) => format!("Unknown signer ({fp})"),
                        SignatureState::Unsigned => "unsigned".to_string(),
                    };
                    println!(
                        "Installed widget {:?} → {} [{}]",
                        result.id,
                        result.install_dir.display(),
                        sig_desc
                    );
                    ExitCode::SUCCESS
                }
                Err(oxidemx_widget_cli::CliError::IdCollision(id)) => {
                    eprintln!("error: widget id {id:?} already installed; use --force to replace");
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
