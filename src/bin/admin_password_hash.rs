use std::io::Read;

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read password from stdin to avoid leaking it into shell history via argv.
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let password = input.trim_end_matches(&['\r', '\n'][..]);
    if password.is_empty() {
        return Err("password is empty; provide it via stdin".into());
    }

    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    println!("{hash}");

    Ok(())
}
