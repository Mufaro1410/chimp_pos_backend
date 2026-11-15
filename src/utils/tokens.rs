use sha2::{Digest, Sha256};
use uuid::Uuid;

/// 256 bits from the OS CSPRNG (two v4 UUIDs, 244 random bits plus fixed bits is ample for an
/// unguessable bearer secret), rendered as 64 hex characters.
pub fn new_opaque_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Hex SHA-256. Tokens are high-entropy, so a fast hash is appropriate (no salt/stretching needed).
pub fn sha256_hex(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
