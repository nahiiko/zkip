//! An end-to-end example of using the SP1 SDK to generate a proof of a program that can have an
//! EVM-Compatible proof generated which can be verified on-chain.
//!
//! You can run this script using the following command:
//! ```shell
//! RUST_LOG=info cargo run --release --bin evm -- --system groth16
//! ```
//! or
//! ```shell
//! RUST_LOG=info cargo run --release --bin evm -- --system plonk
//! ```

use alloy_sol_types::SolType;
use anyhow::{bail, Context};
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use sp1_sdk::{
    include_elf, HashableKey, ProverClient, SP1ProofWithPublicValues, SP1Stdin, SP1VerifyingKey,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use zkip_lib::{ip_to_u32, PublicValuesStruct};

/// The ELF (executable and linkable format) file for the Succinct RISC-V zkVM.
pub const ZKIP_ELF: &[u8] = include_elf!("zkip-program");

/// The arguments for the EVM command.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct EVMArgs {
    /// IP address to test (e.g., "8.8.8.8")
    #[arg(long, default_value = "8.8.8.8")]
    ip: String,

    /// Comma-separated country codes to exclude (e.g., "FR,US,DE")
    #[arg(long, default_value = "FR")]
    exclude: String,

    #[arg(long, value_enum, default_value = "groth16")]
    system: ProofSystem,
}

/// Enum representing the available proof systems
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum ProofSystem {
    Plonk,
    Groth16,
}

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SP1ZkipProofFixture {
    is_excluded: bool,
    timestamp: u32,
    excluded_countries: Vec<u16>,
    vkey: String,
    public_values: String,
    proof: String,
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

    // Parse the command line arguments.
    let args = EVMArgs::parse();

    // Setup the prover client.
    let client = ProverClient::from_env();

    // Setup the program.
    let (pk, vk) = client.setup(ZKIP_ELF);

    // Parse inputs
    let ip = ip_to_u32(&args.ip).context("failed to parse IP address")?;
    let excluded_countries = parse_excluded_countries(&args.exclude)?;

    // TODO: In production, these ranges would come from a GeoIP database
    let excluded_ranges: Vec<(u32, u32)> = vec![
        (ip_to_u32("91.121.0.0")?, ip_to_u32("91.121.31.255")?),
    ];

    let timestamp: u32 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before Unix epoch")?
        .as_secs() as u32;

    // Setup the inputs.
    let mut stdin = SP1Stdin::new();
    stdin.write(&ip);
    stdin.write(&excluded_ranges);
    stdin.write(&excluded_countries);
    stdin.write(&timestamp);

    println!("IP: {} ({})", args.ip, ip);
    println!("Excluded countries: {:?}", excluded_countries);
    println!("Proof System: {:?}", args.system);

    // Generate the proof based on the selected proof system.
    let proof = match args.system {
        ProofSystem::Plonk => client.prove(&pk, &stdin).plonk().run(),
        ProofSystem::Groth16 => client.prove(&pk, &stdin).groth16().run(),
    }
    .context("failed to generate proof")?;

    create_proof_fixture(&proof, &vk, args.system);

    Ok(())
}

/// Create a fixture for the given proof.
fn create_proof_fixture(
    proof: &SP1ProofWithPublicValues,
    vk: &SP1VerifyingKey,
    system: ProofSystem,
) {
    // Deserialize the public values.
    let bytes = proof.public_values.as_slice();
    let PublicValuesStruct {
        is_excluded,
        timestamp,
        excluded_countries,
    } = PublicValuesStruct::abi_decode(bytes).unwrap();

    // Create the testing fixture so we can test things end-to-end.
    let fixture = SP1ZkipProofFixture {
        is_excluded,
        timestamp,
        excluded_countries,
        vkey: vk.bytes32().to_string(),
        public_values: format!("0x{}", hex::encode(bytes)),
        proof: format!("0x{}", hex::encode(proof.bytes())),
    };

    // The verification key is used to verify that the proof corresponds to the execution of the
    // program on the given input.
    println!("Verification Key: {}", fixture.vkey);
    println!("Public Values: {}", fixture.public_values);
    println!("Proof Bytes: {}", fixture.proof);

    // Save the fixture to a file.
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../contracts/src/fixtures");
    std::fs::create_dir_all(&fixture_path).expect("failed to create fixture path");
    std::fs::write(
        fixture_path.join(format!("{:?}-fixture.json", system).to_lowercase()),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .expect("failed to write fixture");
}
