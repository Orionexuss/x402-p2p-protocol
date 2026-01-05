use clap::{Parser, Subcommand};
use std::fs;

#[derive(Parser)]
#[command(name = "x402")]
#[command(about = "x402 P2P protocol CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Inspect {
        file: String,
    },
    Serve {
        #[arg(long, default_value = "0")]
        price: u64,

        #[arg(long)]
        listen: Option<String>,
    },
    Download {
        source: String, // magnet link o .torrent
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { file } => {
            println!("Inspecting torrent file: {}", file);

            // Read the torrent file
            match fs::read(&file) {
                Ok(data) => {
                    if let Err(e) = peer::decode_torrent(&data) {
                        eprintln!("Error decoding torrent: {}", e);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error reading file {}: {}", file, e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Serve { price, listen } => {
            println!(
                "Starting x402 client listening on {:?} with price {}",
                listen, price
            );
        }
        Commands::Download { source } => {
            println!(
                "Downloading files using x402 protocol from source: {}",
                source
            );
        }
    }
}
