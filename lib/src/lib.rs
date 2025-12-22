use alloy_sol_types::sol;
use anyhow::Context;

sol! {
   struct PublicValuesStruct{
    bool is_excluded;
    uint32 timestamp;
    uint16[] excluded_countries;  // ISO 3166-1 numeric codes (840=US, 250=FR, etc.)
   }
}

/// Check if an IP address is excluded from the specified country ranges.
/// Returns true if IP is NOT in any excluded range (user is clear).
/// Returns false if IP IS in an excluded range (user is from blocked country).
pub fn is_excluded(ip: u32, excluded_ranges: Vec<(u32, u32)>) -> bool {
    for (start, end) in excluded_ranges {
        if ip >= start && ip <= end {
            return false;
        }
    }
    true
}

/// Parse an IP address string (e.g., "8.8.8.8") to a u32.
pub fn ip_to_u32(ip_str: &str) -> anyhow::Result<u32> {
    let parts: Vec<&str> = ip_str.split('.').collect();
    if parts.len() != 4 {
        anyhow::bail!("Invalid IP format: expected 4 octets");
    }

    let a: u8 = parts[0].parse().context("Invalid first octet")?;
    let b: u8 = parts[1].parse().context("Invalid second octet")?;
    let c: u8 = parts[2].parse().context("Invalid third octet")?;
    let d: u8 = parts[3].parse().context("Invalid fourth octet")?;

    Ok((a as u32) << 24 | (b as u32) << 16 | (c as u32) << 8 | (d as u32))
}

/// Convert a u32 IP back to dotted string format for display.
pub fn u32_to_ip(ip: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF
    )
}
