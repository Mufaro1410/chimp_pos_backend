fn main() {
    // Re-embed sqlx::migrate!() whenever a migration file is added or changed.
    println!("cargo:rerun-if-changed=migrations");
}
