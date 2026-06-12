use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use oxidemx_widget_cli::{install, pack, verify};
use oxidemx_widget_host::SignatureState;

/// OxideMX widget bundle tool — pack, verify, and install `.omxw` widget bundles.
#[derive(Parser)]
#[command(name = "oxidemx-widget", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pack a widget directory into a signed `.omxw` bundle.
    Pack {
        /// Path to the widget directory (must contain widget.json).
        dir: PathBuf,
        /// Output path for the `.omxw` file (default: `<dir_name>.omxw`).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Verify the signature of a `.omxw` bundle and print the signer fingerprint.
    Verify {
        /// Path to the `.omxw` bundle (legacy `.oxw` accepted).
        bundle: PathBuf,
    },
    /// Install a `.omxw` bundle into the OxideMX widgets directory.
    Install {
        /// Path to the `.omxw` bundle (legacy `.oxw` accepted).
        bundle: PathBuf,
        /// Consent to installing an unsigned/unknown-signer bundle, and
        /// replace an existing widget with the same id.
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
                Err(oxidemx_widget_cli::CliError::NeedsConsent { reason, permissions }) => {
                    match reason {
                        oxidemx_widget_cli::ConsentReason::Unsigned => {
                            eprintln!("refused: bundle has no SIGNATURE entry (unsigned)");
                        }
                        oxidemx_widget_cli::ConsentReason::UnknownKey(fp) => {
                            eprintln!(
                                "refused: bundle is signed by an unknown key \
                                 (fingerprint: {fp}), not the OxideMX registry key"
                            );
                        }
                    }
                    if permissions.is_empty() {
                        eprintln!("the widget's manifest declares no permissions");
                    } else {
                        eprintln!("the widget's manifest declares these permissions:");
                        for p in &permissions {
                            eprintln!("  - {p}");
                        }
                    }
                    eprintln!(
                        "permissions are self-declared by the widget author; \
                         re-run with --force to install anyway"
                    );
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
