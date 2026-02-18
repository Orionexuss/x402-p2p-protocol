use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use x402_core::torrent::parser::calculate_info_hash;

#[derive(Parser)]
#[command(name = "x402")]
#[command(about = "x402 P2P protocol CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Serialize, Deserialize)]
struct SeederConfig {
    info_hashes: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    Create {
        /// Path to the file to create a torrent for
        file: String,

        /// Tracker announce URL
        #[arg(long, short = 't', default_value = "http://localhost:6969/announce")]
        tracker: String,

        /// Output .torrent file path (optional, defaults to <filename>.torrent)
        #[arg(long, short = 'o')]
        output: Option<String>,

        /// Generate magnet URI instead of .torrent file
        #[arg(long, short = 'm')]
        magnet: bool,
    },
    Inspect {
        file: String,
    },
    Serve {
        #[arg(long, default_value = "0")]
        price: u64,

        #[arg(long)]
        listen: Option<String>,

        #[arg(long, short = 'c', default_value = "seeder.json")]
        config: String,

        #[arg(long, default_value = "http://localhost:6969")]
        tracker: String,
    },
    Download {
        source: String, // magnet link or .torrent file

        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    Tracker {
        #[arg(long, default_value = "0.0.0.0:6969")]
        listen: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Create {
            file,
            tracker,
            output,
            magnet,
        } => {
            println!("Creating torrent from file: {}", file);
            println!("Tracker: {}", tracker);
            println!();

            // Build torrent with automatic piece length calculation
            let builder = x402_core::TorrentBuilder::new(&file, &tracker);

            if magnet {
                // Generate magnet URI
                match builder.build_magnet() {
                    Ok(magnet_link) => {
                        let magnet_url = magnet_link.to_url();
                        println!("Magnet URI generated:");
                        println!("{}", magnet_url);
                        println!();
                        println!("Info Hash: {}", magnet_link.info_hash);
                        if let Some(name) = &magnet_link.display_name {
                            println!("Name: {}", name);
                        }
                        if let Some(length) = magnet_link.exact_length {
                            println!("Size: {} bytes", length);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error generating magnet URI: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                // Create .torrent file
                let output_path = output.unwrap_or_else(|| {
                    format!(
                        "{}.torrent",
                        file.split('/').next_back().unwrap_or("output")
                    )
                });

                match builder.build_to_file(&output_path) {
                    Ok(_) => {
                        println!("Torrent file created: {}", output_path);

                        // Also calculate and display info hash
                        if let Ok(info_hash) = builder.build_info_hash() {
                            println!("Info Hash: {}", hex::encode(info_hash));
                        }
                    }
                    Err(e) => {
                        eprintln!("Error creating torrent file: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
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
        Commands::Serve {
            price,
            listen,
            config,
            tracker,
        } => {
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

            let mut seeder = x402_core::Seeder::new(addr, port);

            // Load config file
            println!("Loading config from: {}", config);
            match fs::read_to_string(&config) {
                Ok(content) => match serde_json::from_str::<SeederConfig>(&content) {
                    Ok(cfg) => {
                        println!("Found {} info hashes in config", cfg.info_hashes.len());
                        for info_hash_hex in &cfg.info_hashes {
                            match seeder.add_torrent_hex(info_hash_hex) {
                                Ok(_) => println!("  Added: {}", info_hash_hex),
                                Err(e) => eprintln!("  Error adding {}: {}", info_hash_hex, e),
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error parsing config: {}", e);
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("Error reading config file: {}", e);
                    eprintln!("Create a config file '{}' with format:", config);
                    eprintln!("{{\"info_hashes\": [\"<40-char-hex>\"]}}");
                    std::process::exit(1);
                }
            }

            // Announce all torrents to tracker
            if !seeder.info_hashes.is_empty() {
                println!("\nAnnouncing to tracker: {}", tracker);
                for info_hash in &seeder.info_hashes {
                    match seeder
                        .announce_to_tracker(tracker.clone(), *info_hash)
                        .await
                    {
                        Ok(response) => {
                            println!(
                                "  Announced {} - {} seeders, {} leechers",
                                hex::encode(info_hash),
                                response.seeders.len(),
                                response.leechers.len()
                            );
                        }
                        Err(e) => {
                            eprintln!("  Failed to announce {}: {}", hex::encode(info_hash), e);
                        }
                    }
                }
            } else {
                println!("\nNo torrents to announce (config is empty)");
            }

            println!("\nStarting listener...");
            if let Err(e) = seeder.listen() {
                eprintln!("Error starting seeder: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Download { source, output } => {
            println!("x402 Download");
            println!("Source: {}", source);
            println!();
            let mut tracker;

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
                        println!("Trackers: {},", magnet.trackers.len());
                        println!();

                        tracker = magnet.trackers.first().cloned().unwrap_or_default();

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
                            println!("  Tracker: {}", torrent.announce);
                            println!();

                            tracker = torrent.announce.clone();

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

            // remove /announce from tracker URL if present
            if tracker.ends_with("/announce") {
                tracker = tracker.trim_end_matches("/announce").to_string();
            }

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
        Commands::Tracker { listen } => {
            println!("Starting x402 tracker server on {}", listen);

            // The tracker binary needs to be run separately
            // This command will call the tracker executable
            let status = std::process::Command::new("cargo")
                .args(["run", "--bin", "tracker", "--release"])
                .env("TRACKER_LISTEN", listen)
                .status();

            match status {
                Ok(exit_status) => {
                    if !exit_status.success() {
                        eprintln!("Tracker server exited with status: {}", exit_status);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error starting tracker: {}", e);
                    eprintln!("Make sure the tracker binary is built: cargo build --bin tracker");
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
}
