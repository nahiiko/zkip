use alloy_sol_types::sol;

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
