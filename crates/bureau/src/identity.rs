//! Operating-system random identities for durable cross-host coordination.

use std::io::Read as _;

pub fn random_hex() -> std::io::Result<String> {
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(format!("{:032x}", u128::from_le_bytes(bytes)))
}
