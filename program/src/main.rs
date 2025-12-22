//! zkip - Zero-knowledge IP location proof
//! Proves an IP is NOT from specified countries without revealing the IP.

#![no_main]
sp1_zkvm::entrypoint!(main);

use alloy_sol_types::SolType;
use zkip_lib::{is_excluded, PublicValuesStruct};

pub fn main() {
    // Read private inputs
    let ip = sp1_zkvm::io::read::<u32>();
    let excluded_ranges = sp1_zkvm::io::read::<Vec<(u32, u32)>>();

    // Read public inputs
    let excluded_countries = sp1_zkvm::io::read::<Vec<u16>>();
    let timestamp = sp1_zkvm::io::read::<u32>();

    // Check if IP is NOT in any excluded range
    let is_excluded = is_excluded(ip, excluded_ranges);

    // Encode the public values of the program.
    let bytes = PublicValuesStruct::abi_encode(&PublicValuesStruct {
        is_excluded,
        timestamp,
        excluded_countries,
    });

    // Commit to the public values of the program. The final proof will have a commitment to all the
    // bytes that were committed to.
    sp1_zkvm::io::commit_slice(&bytes);
}
