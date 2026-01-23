use clap::{Parser, Subcommand};
use std::fs;
use x402_core::torrent::parser::calculate_info_hash;

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
        source: String, // magnet link or .torrent file

        #[arg(long, default_value = "http://localhost:6969")]
        tracker: String,

        #[arg(long, short = 'o')]
        output: Option<String>,
    },
}

#[tokio::main]
async fn main() {
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
                (
                    parts[0].to_string(),
                    parts[1].parse::<u16>().unwrap_or(6881),
                )
            } else {
                ("0.0.0.0".to_string(), 6881)
            };

            println!(
                "Starting x402 seeder on {}:{} with price {}",
                addr, port, price
            );

            let seeder = x402_core::Seeder::new(addr, port);

            // TODO: Load torrents from config/database
            // For now, you need to add torrents manually
            println!("Note: Add torrents to seed using seeder.add_torrent_hex()");

            if let Err(e) = seeder.listen() {
                eprintln!("Error starting seeder: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Download {
            source,
            tracker,
            output,
        } => {
            println!("x402 Download");
            println!("Source: {}", source);
            println!("Tracker: {}", tracker);
            println!();

            // Parse source (magnet or .torrent file)
            let (info_hash, file_name, total_size) = if source.starts_with("magnet:?") {
                // Parse magnet link
                match x402_core::MagnetLink::parse(&source) {
                    Ok(magnet) => {
                        println!("Parsed magnet link:");
                        println!("  Info Hash: {}", magnet.info_hash);
                        if let Some(name) = &magnet.display_name {
                            println!("  Name: {}", name);
                        }
                        if let Some(length) = magnet.exact_length {
                            println!("  Size: {} bytes", length);
                        }
                        println!();

                        // Decode hex info hash
                        let info_hash_bytes = hex::decode(&magnet.info_hash)
                            .expect("Invalid info hash in magnet link");
                        let mut info_hash = [0u8; 20];
                        info_hash.copy_from_slice(&info_hash_bytes);

                        (
                            info_hash,
                            magnet
                                .display_name
                                .unwrap_or_else(|| "download".to_string()),
                            magnet.exact_length.unwrap_or(0),
                        )
                    }
                    Err(e) => {
                        eprintln!("Error parsing magnet link: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                // Parse .torrent file
                match fs::read(&source) {
                    Ok(data) => match x402_core::decode_torrent(&data) {
                        Ok(torrent) => {
                            println!("Parsed torrent file:");
                            println!("  Name: {}", torrent.info.name);
                            println!("  Pieces: {}", torrent.info.pieces.len() / 20);
                            println!("  Total Size: {} bytes", torrent.total_length());
                            println!();

                            let torrent_length = torrent.total_length();
                            let info_hash = calculate_info_hash(&torrent);

                            (info_hash, torrent.info.name, torrent_length)
                        }
                        Err(e) => {
                            eprintln!("Error decoding torrent: {}", e);
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("Error reading file {}: {}", source, e);
                        std::process::exit(1);
                    }
                }
            };

            // Determine output path
            let output_path = output.unwrap_or_else(|| file_name.clone());

            // Create leecher and start download
            let leecher = x402_core::Leecher::new(
                info_hash,
                tracker,
                std::path::PathBuf::from(output_path),
                total_size,
            );

            if let Err(e) = leecher.download().await {
                eprintln!("Download failed: {}", e);
                std::process::exit(1);
            }
        }
    }
}
