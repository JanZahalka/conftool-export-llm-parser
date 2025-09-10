use clap::{Parser, Subcommand};
use dotenv::dotenv;

mod mandatory_reviewers;
mod openai;

#[derive(Parser)]
#[command(name = "conftool_helper")]
#[command(about = "A CLI helper for ConfTool data processing")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Parse and process mandatory reviewers data")]
    MandatoryReviewers {
        /// Overwrite existing output files
        #[arg(long)]
        overwrite: bool,
    },
}

fn main() {
    // Load environment vars
    dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::MandatoryReviewers { overwrite } => {
            if let Err(e) = mandatory_reviewers::parse_mandatory_reviewers(overwrite) {
                eprintln!("Error processing mandatory reviewers: {}", e);
                std::process::exit(1);
            }
        }
    }
}
