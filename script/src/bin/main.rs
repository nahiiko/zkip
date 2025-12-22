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
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};
use zkip_lib::{ip_to_u32, PublicValuesStruct};

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const ZKIP_ELF: &[u8] = include_elf!("zkip-program");

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
}

/// Load country codes from CSV file.
/// Returns a map of alpha-2 code -> numeric code (e.g., "FR" -> 250)
fn load_country_codes() -> anyhow::Result<HashMap<String, u16>> {
    let csv_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/countries.csv");
    let file = File::open(csv_path).context("Failed to open countries.csv")?;
    let reader = BufReader::new(file);

    let mut codes = HashMap::new();
    for (i, line) in reader.lines().enumerate() {
        if i == 0 {
            continue; // Skip header
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
fn parse_excluded_countries(exclude_arg: &str) -> anyhow::Result<Vec<u16>> {
    let country_codes = load_country_codes()?;
    let mut result = Vec::new();

    for code in exclude_arg.split(',') {
        let code = code.trim().to_uppercase();
        if code.is_empty() {
            continue;
        }
        match country_codes.get(&code) {
            Some(&numeric) => result.push(numeric),
            None => bail!("Unknown country code: {}", code),
        }
    }

    if result.is_empty() {
        bail!("No valid country codes provided");
    }

    Ok(result)
}

fn main() -> anyhow::Result<()> {
    // Setup the logger.
    sp1_sdk::utils::setup_logger();
    dotenv::dotenv().ok();

    // Parse the command line arguments.
    let args = Args::parse();

    if args.execute == args.prove {
        eprintln!("Error: You must specify either --execute or --prove");
        std::process::exit(1);
    }

    // Setup the prover client.
    let client = ProverClient::from_env();

    // Parse CLI arguments
    let ip = ip_to_u32(&args.ip).context("failed to parse IP address")?;
    let excluded_countries = parse_excluded_countries(&args.exclude)?;

    // TODO: In production, these ranges would come from a GeoIP database
    // For now, using a hardcoded France IP range (OVH: 91.121.0.0 - 91.121.31.255)
    let excluded_ranges: Vec<(u32, u32)> = vec![
        (ip_to_u32("91.121.0.0")?, ip_to_u32("91.121.31.255")?),
    ];

    let timestamp: u32 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before Unix epoch")?
        .as_secs() as u32;

    // Write inputs to stdin (must match order in program!)
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
        // Execute the program
        let (output, report) = client
            .execute(ZKIP_ELF, &stdin)
            .run()
            .context("failed to execute zkvm program")?;
        println!("Program executed successfully.");

        // Read the output.
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

        // Verify against local computation
        let expected = zkip_lib::is_excluded(ip, excluded_ranges.clone());
        assert_eq!(is_excluded, expected);
        println!("Verification passed!");

        // Record the number of cycles executed.
        println!("Number of cycles: {}", report.total_instruction_count());
    } else {
        // Setup the program for proving.
        let (pk, vk) = client.setup(ZKIP_ELF);

        // Generate the proof
        let proof = client
            .prove(&pk, &stdin)
            .run()
            .context("failed to generate proof")?;

        println!("Successfully generated proof!");

        // Verify the proof.
        client.verify(&proof, &vk).context("failed to verify proof")?;
        println!("Successfully verified proof!");
    }
    Ok(())
}
