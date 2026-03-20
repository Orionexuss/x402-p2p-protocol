use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use x402_core::torrent::parser::calculate_info_hash;

const SEEDER_CONFIG_PATH: &str = "seeder.json";

#[derive(Debug, Deserialize)]
struct SeederConfig {
    info_hashes: Vec<HashMap<String, serde_json::Value>>,
}

fn load_seeder_prices(config_path: &Path) -> Result<HashMap<[u8; 20], u64>, String> {
    let config_data = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path.display(), e))?;
    let config: SeederConfig = serde_json::from_str(&config_data)
        .map_err(|e| format!("Invalid {} format: {}", config_path.display(), e))?;

    let mut prices_by_info_hash = HashMap::new();

    for entry in config.info_hashes {
        if entry.len() != 1 {
            return Err(
                "Each item in seeder.json info_hashes must contain exactly one info hash to price mapping"
                    .to_string(),
            );
        }

        let (info_hash_hex, raw_price) = entry.into_iter().next().unwrap();
        let info_hash = parse_info_hash(&info_hash_hex)?;

        let price_str = raw_price.as_str().ok_or_else(|| {
            format!(
                "Price for info hash {} must be a string like \"5.00\"; JSON numbers do not preserve trailing zeros",
                info_hash_hex
            )
        })?;

        let price = parse_price_to_minor_units(price_str)?;

        if prices_by_info_hash.insert(info_hash, price).is_some() {
            return Err(format!(
                "Duplicate price entry found for info hash {}",
                info_hash_hex
            ));
        }
    }

    Ok(prices_by_info_hash)
}

fn parse_info_hash(info_hash_hex: &str) -> Result<[u8; 20], String> {
    if info_hash_hex.len() != 40 {
        return Err(format!(
            "Invalid info hash {}: expected 40 hex characters",
            info_hash_hex
        ));
    }

    let decoded = hex::decode(info_hash_hex)
        .map_err(|e| format!("Invalid info hash {}: {}", info_hash_hex, e))?;

    let mut info_hash = [0u8; 20];
    info_hash.copy_from_slice(&decoded);
    Ok(info_hash)
}

fn parse_price_to_minor_units(price: &str) -> Result<u64, String> {
    let (whole, fractional) = price
        .split_once('.')
        .ok_or_else(|| format!("Invalid price {}: expected format X.YY", price))?;

    if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid price {}: whole part must be digits",
            price
        ));
    }

    if fractional.len() != 2 || !fractional.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Invalid price {}: fractional part must contain exactly two digits",
            price
        ));
    }

    let whole = whole
        .parse::<u64>()
        .map_err(|e| format!("Invalid price {}: {}", price, e))?;
    let fractional = fractional
        .parse::<u64>()
        .map_err(|e| format!("Invalid price {}: {}", price, e))?;

    whole
        .checked_mul(100)
        .and_then(|value| value.checked_add(fractional))
        .ok_or_else(|| format!("Invalid price {}: value is too large", price))
}

fn format_minor_units(price: u64) -> String {
    format!("{}.{:02}", price / 100, price % 100)
}

#[derive(Parser)]
#[command(name = "x402")]
#[command(about = "x402 P2P protocol CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        #[arg(long)]
        listen: Option<String>,

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
                    Ok(data) => match x402_core::decode_torrent(&data) {
                        Ok(torrent) => {
                            println!("Name: {}", torrent.info.name);
                            println!("Piece Length: {} bytes", torrent.info.plength);
                            println!("Total Length: {} bytes", torrent.total_length());
                            println!("Number of Pieces: {}", torrent.info.pieces.len() / 20);
                            println!("Trackers: {}", torrent.announce);
                            let info_hash = calculate_info_hash(&torrent);
                            println!("Info Hash: {}", hex::encode(info_hash));
                        }
                        Err(e) => {
                            eprintln!("Error decoding torrent file: {}", e);
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("Error reading file {}: {}", file, e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Commands::Serve { listen, tracker } => {
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

            println!("Starting x402 seeder on {}:{}", addr, port);

            let seeder_prices =
                load_seeder_prices(Path::new(SEEDER_CONFIG_PATH)).unwrap_or_else(|e| {
                    eprintln!("Seeder price configuration error: {}", e);
                    std::process::exit(1);
                });

            let (mut seeder, torrent_manager) = x402_core::Seeder::new(addr, port).unwrap();

            println!("Serving these info hashes:");
            for hash in &seeder.info_hashes {
                println!("  {}", hex::encode(hash));
            }

            // Announce all torrents to tracker
            if !seeder.info_hashes.is_empty() {
                println!("\nAnnouncing to tracker: {}", tracker);
                let info_hashes = seeder.info_hashes.clone();
                for info_hash in info_hashes {
                    let price = *seeder_prices.get(&info_hash).unwrap_or_else(|| {
                        eprintln!(
                            "Missing price in {} for info hash {}",
                            SEEDER_CONFIG_PATH,
                            hex::encode(info_hash)
                        );
                        std::process::exit(1);
                    });

                    match seeder
                        .announce_to_tracker(tracker.clone(), price, info_hash)
                        .await
                    {
                        Ok(response) => {
                            println!(
                                "  Announced {} at price {} - {} seeders, {} leechers",
                                hex::encode(info_hash),
                                format_minor_units(price),
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
            if let Err(e) = seeder.listen(&torrent_manager) {
                eprintln!("Error starting seeder: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Download { source, output } => {
            println!("x402 Download");
            println!("Source: {}", source);
            println!();
            let mut tracker;
            let is_magnet: bool;

            // Parse source (magnet or .torrent file)
            let (info_hash, file_name, total_size) = if source.starts_with("magnet:?") {
                // Parse magnet link
                is_magnet = true;

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
                is_magnet = false;
                // Parse .torrent file
                match fs::read(&source) {
                    Ok(data) => match x402_core::decode_torrent(&data) {
                        Ok(torrent) => {
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

            if let Err(e) = leecher.download(is_magnet).await {
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

#[cfg(test)]
mod tests {
    use super::parse_price_to_minor_units;

    #[test]
    fn parses_valid_two_decimal_prices() {
        assert_eq!(parse_price_to_minor_units("5.00").unwrap(), 500);
        assert_eq!(parse_price_to_minor_units("1.24").unwrap(), 124);
        assert_eq!(parse_price_to_minor_units("529.43").unwrap(), 52943);
    }

    #[test]
    fn rejects_invalid_price_formats() {
        assert!(parse_price_to_minor_units("5").is_err());
        assert!(parse_price_to_minor_units("5.0").is_err());
        assert!(parse_price_to_minor_units("5.000").is_err());
        assert!(parse_price_to_minor_units("abc").is_err());
    }
}
