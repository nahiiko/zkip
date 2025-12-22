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
use anyhow::Context;
use clap::Parser;
use sp1_sdk::{include_elf, ProverClient, SP1Stdin};
use std::time::{SystemTime, UNIX_EPOCH};
use zkip_lib::PublicValuesStruct;

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

    // Setup test inputs
    // US IP - Google DNS (should return true = excluded from France)
    let ip: u32 = 134_744_072; // 8.8.8.8
                               // France IP - OVH (should return false = NOT excluded, IS in France)
                               // let ip: u32 = 1_534_132_225;  // 91.121.0.1

    // France IP range (OVH: 91.121.0.0 - 91.121.31.255)
    let excluded_ranges: Vec<(u32, u32)> = vec![(1_534_132_224, 1_534_140_415)];

    // Public inputs
    let excluded_countries: Vec<u16> = vec![250]; // France ISO code
    let timestamp: u32 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System clock is before Unix epoch")
        .as_secs() as u32;

    // Write inputs to stdin (must match order in program!)
    let mut stdin = SP1Stdin::new();
    stdin.write(&ip);
    stdin.write(&excluded_ranges);
    stdin.write(&excluded_countries);
    stdin.write(&timestamp);

    println!(
        "Testing IP: {} against excluded countries: {:?}",
        ip, excluded_countries
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
