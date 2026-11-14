use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::{SaltString, rand_core::OsRng},
};

fn argon2_instance() -> Result<Argon2<'static>, String> {
    let params = Params::new(
        19 * 1024, // 19 MiB memory
        2,         // 2 iterations
        1,         // 1 degree of parallelism
        None,
    )
    .map_err(|err| err.to_string())?;

    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = argon2_instance()?;

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| err.to_string())?;

    Ok(password_hash.to_string())
}

pub fn verify_password(password: &str, stored_hash: &str) -> Result<bool, String> {
    let parsed_hash = PasswordHash::new(stored_hash).map_err(|err| err.to_string())?;

    let argon2 = argon2_instance()?;

    match argon2.verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(err) => Err(err.to_string()),
    }
}
