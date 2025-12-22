//! An end-to-end example of using the SP1 SDK to generate a proof of a program that can be executed
//! or have a core proof generated.
//!
//! You can run this script using the following command:
//! ```shell
//! RUST_LOG=info cargo run --release -- --execute
//! ```
//! or
//! ```shell
//! RUST_LOG=info cargo run --release -- --prove
//! ```

use alloy_sol_types::SolType;
use anyhow::{bail, Context};
use clap::Parser;
use sp1_sdk::{include_elf, ProverClient, SP1Stdin};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zkip_lib::{ip_to_u32, PublicValuesStruct};

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const ZKIP_ELF: &[u8] = include_elf!("zkip-program");

const GEOIP_URL: &str = "https://cdn.jsdelivr.net/npm/@ip-location-db/geo-whois-asn-country/geo-whois-asn-country-ipv4-num.csv";
const CACHE_MAX_AGE_DAYS: u32 = 30;

/// The arguments for the command.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    execute: bool,

    #[arg(long)]
    prove: bool,

    /// IP address to test (e.g., "8.8.8.8")
    #[arg(long, default_value = "8.8.8.8")]
    ip: String,

    /// Comma-separated country codes to exclude (e.g., "FR,US,DE")
    #[arg(long, default_value = "FR")]
    exclude: String,

    /// Force refresh the GeoIP database
    #[arg(long)]
    refresh: bool,
}

fn get_cache_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/ipv4-country.csv")
}

fn is_cache_stale(path: &PathBuf) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = metadata.modified() else {
        return true;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        return true;
    };
    age > Duration::from_secs((CACHE_MAX_AGE_DAYS * 24 * 60 * 60) as u64)
}

fn fetch_geoip_database(path: &PathBuf) -> anyhow::Result<()> {
    println!("Fetching GeoIP database from {}...", GEOIP_URL);

    let response = reqwest::blocking::get(GEOIP_URL)
        .context("Failed to fetch GeoIP database")?;

    if !response.status().is_success() {
        bail!("HTTP error: {}", response.status());
    }

    let content = response.text().context("Failed to read response")?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create data directory")?;
    }

    let mut file = File::create(path).context("Failed to create cache file")?;
    file.write_all(content.as_bytes()).context("Failed to write cache file")?;

    println!("GeoIP database cached to {:?}", path);
    Ok(())
}

fn ensure_geoip_database(refresh: bool) -> anyhow::Result<PathBuf> {
    let path = get_cache_path();

    if refresh || !path.exists() || is_cache_stale(&path) {
        let reason = if refresh {
            "refresh requested"
        } else if !path.exists() {
            "cache not found"
        } else {
            "cache older than 30 days"
        };
        println!("Updating GeoIP database ({})...", reason);

        if let Err(e) = fetch_geoip_database(&path) {
            if path.exists() {
                eprintln!("Warning: Failed to fetch GeoIP database: {}. Using cached version.", e);
            } else {
                return Err(e);
            }
        }
    }

    Ok(path)
}

/// Load country codes from CSV file.
fn load_country_codes() -> anyhow::Result<HashMap<String, u16>> {
    let csv_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/countries.csv");
    let file = File::open(csv_path).context("Failed to open countries.csv")?;
    let reader = BufReader::new(file);

    let mut codes = HashMap::new();
    for (i, line) in reader.lines().enumerate() {
        if i == 0 {
            continue;
        }
        let line = line.context("Failed to read line")?;
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 4 {
            let alpha2 = fields[1].to_uppercase();
            if let Ok(numeric) = fields[3].parse::<u16>() {
                codes.insert(alpha2, numeric);
            }
        }
    }
    Ok(codes)
}

/// Parse comma-separated country codes and resolve to numeric codes.
fn parse_excluded_countries(exclude_arg: &str) -> anyhow::Result<(Vec<String>, Vec<u16>)> {
    let country_codes = load_country_codes()?;
    let mut alpha2_codes = Vec::new();
    let mut numeric_codes = Vec::new();

    for code in exclude_arg.split(',') {
        let code = code.trim().to_uppercase();
        if code.is_empty() {
            continue;
        }
        match country_codes.get(&code) {
            Some(&numeric) => {
                alpha2_codes.push(code);
                numeric_codes.push(numeric);
            }
            None => bail!("Unknown country code: {}", code),
        }
    }

    if numeric_codes.is_empty() {
        bail!("No valid country codes provided");
    }

    Ok((alpha2_codes, numeric_codes))
}

/// Load IPv4 ranges for specified countries from the GeoIP database.
fn load_ip_ranges_for_countries(path: &PathBuf, country_codes: &[String]) -> anyhow::Result<Vec<(u32, u32)>> {
    let file = File::open(path).context("Failed to open GeoIP database")?;
    let reader = BufReader::new(file);

    let mut ranges = Vec::new();
    for line in reader.lines() {
        let line = line.context("Failed to read line")?;
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 3 {
            let country = fields[2].to_uppercase();
            if country_codes.contains(&country) {
                let start: u32 = fields[0].parse().context("Invalid start IP")?;
                let end: u32 = fields[1].parse().context("Invalid end IP")?;
                ranges.push((start, end));
            }
        }
    }

    Ok(ranges)
}

fn main() -> anyhow::Result<()> {
    sp1_sdk::utils::setup_logger();
    dotenv::dotenv().ok();

    let args = Args::parse();

    if args.execute == args.prove {
        eprintln!("Error: You must specify either --execute or --prove");
        std::process::exit(1);
    }

    // Ensure GeoIP database is available and fresh
    let geoip_path = ensure_geoip_database(args.refresh)?;

    let client = ProverClient::from_env();

    let ip = ip_to_u32(&args.ip).context("failed to parse IP address")?;
    let (alpha2_codes, excluded_countries) = parse_excluded_countries(&args.exclude)?;

    let excluded_ranges = load_ip_ranges_for_countries(&geoip_path, &alpha2_codes)?;
    println!("Loaded {} IP ranges for {:?}", excluded_ranges.len(), alpha2_codes);

    let timestamp: u32 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before Unix epoch")?
        .as_secs() as u32;

    let mut stdin = SP1Stdin::new();
    stdin.write(&ip);
    stdin.write(&excluded_ranges);
    stdin.write(&excluded_countries);
    stdin.write(&timestamp);

    println!(
        "Testing IP: {} ({}) against excluded countries: {:?}",
        args.ip, ip, excluded_countries
    );

    if args.execute {
        let (output, report) = client
            .execute(ZKIP_ELF, &stdin)
            .run()
            .context("failed to execute zkvm program")?;
        println!("Program executed successfully.");

        let decoded = PublicValuesStruct::abi_decode(output.as_slice())
            .context("failed to decode public values")?;
        let PublicValuesStruct {
            is_excluded,
            timestamp,
            excluded_countries,
        } = decoded;

        println!("Result: is_excluded = {}", is_excluded);
        println!("Timestamp: {}", timestamp);
        println!("Checked countries: {:?}", excluded_countries);

        let expected = zkip_lib::is_excluded(ip, excluded_ranges.clone());
        assert_eq!(is_excluded, expected);
        println!("Verification passed!");

        println!("Number of cycles: {}", report.total_instruction_count());
    } else {
        let (pk, vk) = client.setup(ZKIP_ELF);

        let proof = client
            .prove(&pk, &stdin)
            .run()
            .context("failed to generate proof")?;

        println!("Successfully generated proof!");

        client.verify(&proof, &vk).context("failed to verify proof")?;
        println!("Successfully verified proof!");
    }
    Ok(())
}
