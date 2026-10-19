//! Legt die Demo-Datenbank `examples/data/demo.kdbx` an (Master-Passwort: demo1234).
//!
//! Ausfuehren mit: `cargo run --example make_demo_db`

use std::fs::{self, File};
use std::path::PathBuf;

use keepass::{Database, DatabaseKey};

pub const DEMO_DB_PASSWORD: &str = "demo1234";

pub fn demo_db_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/data/demo.kdbx")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = Database::new();

    let entries = [
        ("GitHub", "peter", "https://github.com", "gh-Geheim-42"),
        ("Mailserver", "peter@example.org", "https://mail.example.org", "M@il!2026"),
        ("Bankkonto", "peter.mustermann", "https://bank.example.org", "Konto#9876"),
        ("Router Admin", "admin", "http://192.168.1.1", "router-pw"),
        ("MVS Testsystem", "IBMUSER", "tn3270://localhost:3271", "SYS1pass"),
    ];

    for (title, user, url, pw) in entries {
        db.root_mut().add_entry().edit(|e| {
            e.set_unprotected("Title", title);
            e.set_unprotected("UserName", user);
            e.set_unprotected("URL", url);
            e.set_protected("Password", pw);
        });
    }

    let path = demo_db_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = File::create(&path)?;
    db.save(&mut file, DatabaseKey::new().with_password(DEMO_DB_PASSWORD))?;
    println!("Demo-Datenbank geschrieben: {}", path.display());
    Ok(())
}
