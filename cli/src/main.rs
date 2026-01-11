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
            // Check if it's a magnet link or a .torrent file
            if file.starts_with("magnet:?") {
                println!("Inspecting magnet link...");
                match x402_core::MagnetLink::parse(&file) {
                    Ok(magnet) => {
                        println!("Info Hash: {}", magnet.info_hash);
                        if let Some(name) = &magnet.display_name {
                            println!("Name: {}", name);
                        }
                        if !magnet.trackers.is_empty() {
                            println!("Trackers:");
                            for tracker in &magnet.trackers {
                                println!("  - {}", tracker);
                            }
                        }
                        if let Some(length) = magnet.exact_length {
                            println!("Size: {} bytes", length);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error parsing magnet link: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                println!("Inspecting torrent file: {}", file);
                // Read the torrent file
                match fs::read(&file) {
                    Ok(data) => {
                        if let Err(e) = x402_core::decode_torrent(&data) {
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
        }
        Commands::Serve { price, listen } => {
            let address = listen.unwrap_or_else(|| "0.0.0.0:6881".to_string());
            let parts: Vec<&str> = address.split(':').collect();
            
            let (addr, port) = if parts.len() == 2 {
                (parts[0].to_string(), parts[1].parse::<u16>().unwrap_or(6881))
            } else {
                ("0.0.0.0".to_string(), 6881)
            };

            println!("Starting x402 seeder on {}:{} with price {}", addr, port, price);
            
            let mut seeder = x402_core::Seeder::new(addr, port);
            
            // TODO: Load torrents from config/database
            // For now, you need to add torrents manually
            println!("Note: Add torrents to seed using seeder.add_torrent_hex()");
            
            if let Err(e) = seeder.listen() {
                eprintln!("Error starting seeder: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Download { source } => {
            println!(
                "Downloading files using x402 protocol from source: {}",
                source
            );
        }
    }
}
