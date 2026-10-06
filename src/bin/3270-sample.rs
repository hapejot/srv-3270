use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use srv_3270::*;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------
fn login_screen(width: u16, username_prefill: &str, error: Option<&str>) -> DataStream {
    let hint = error
        .unwrap_or("Enter=Anmelden  Clear/PF3=Abbrechen")
        .to_string();

    DataStream {
        command: Command::EraseWriteAlternate,
        wcc: Wcc::UnlockKeyboardResetMdt,
        width,
        orders: vec![
            Order::SetBufferAddress { row: 1, col: 1 },
            Order::StartField(FieldAttribute::unprotected_normal()),
            Order::InsertCursor,
            Order::SetAttribute(Attribute::Highlighting(Highlight::Underscore)),
            Order::Nulls(4),
            Order::StartField(FieldAttribute::protected_intensified()),
            Order::SetAttribute(Attribute::Foreground(Color::NeutralWhite)),
            Order::RepeatToAddress {
                row: 1,
                col: 18,
                fill: ' ',
            },
            Order::Text("INTERACTIVE MONITORING PROGRAM FOR MVS/370".into()),
            // -- Menuepunkt "A - ADDRESS SPACE MONITOR" --
            Order::SetBufferAddress { row: 3, col: 5 },
            Order::StartField(FieldAttribute::protected_intensified()),
            Order::SetAttribute(Attribute::Foreground(Color::Yellow)),
            Order::Text("A".into()),
            Order::StartField(FieldAttribute::protected_detectable()),
            Order::SetAttribute(Attribute::Foreground(Color::Turquoise)),
            Order::Text("-".into()),
            Order::StartField(FieldAttribute::protected_intensified()),
            Order::SetAttribute(Attribute::Foreground(Color::Pink)),
            Order::Text("ADDRESS SPACE".into()),
            Order::SetBufferAddress { row: 6, col: 5 },
            Order::StartField(FieldAttribute::protected_normal()),
            Order::Text("Benutzer:".into()),
            Order::SetBufferAddress { row: 6, col: 20 },
            Order::StartField(FieldAttribute::unprotected_normal()),
            Order::Nulls(20),
            Order::StartField(FieldAttribute::protected_normal()),
            Order::Text(username_prefill.to_string()),
            Order::SetBufferAddress { row: 8, col: 5 },
            Order::StartField(FieldAttribute::protected_normal()),
            Order::Text("Passwort:".into()),
            Order::SetBufferAddress { row: 8, col: 20 },
            Order::StartField(FieldAttribute::unprotected_hidden()),
            Order::Nulls(20),
            Order::StartField(FieldAttribute::protected_normal()),
            Order::SetBufferAddress { row: 22, col: 5 },
            Order::StartField(FieldAttribute::protected_normal()),
            Order::Text(hint),
            // Order::SetBufferAddress { row: 6, col: 21 },
            // Order::InsertCursor,
        ],
    }
}

fn result_screen(width: u16, username: &str) -> DataStream {
    DataStream {
        command: Command::EraseWrite,
        wcc: Wcc::UnlockKeyboardResetMdt,
        width,
        orders: vec![
            Order::SetBufferAddress { row: 2, col: 5 },
            Order::StartField(FieldAttribute::protected_normal()),
            Order::Text(format!("Hallo {}, Login empfangen.", username)),
        ],
    }
}

struct AppState {
    inner: Arc<Mutex<AppStateInner>>,
}

struct AppStateInner {
    screens: Screens,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Kurze Demonstration: der Login-Screen als JSON -- zeigt, dass
    // DataStream/Order jetzt eine vollstaendig beschreibbare (und damit
    // auch loggbare/testbare) Struktur sind statt roher Bytes.
    // let demo = login_screen(80, "", None);
    // println!(
    //     "Beispiel-Datenstrom als JSON:\n{}\n",
    //     serde_json::to_string_pretty(&demo).unwrap()
    // );

    let listener = TcpListener::bind("0.0.0.0:3270").await?;
    println!("TN3270-Demo-Server laeuft auf Port 3270 ...");

    let screens = Screens::new();

    loop {
        let (stream, _) = listener.accept().await?;
        let screens = screens.clone();
        tokio::spawn(async move {
            let mut context = Context::new();
            if let Err(e) = handle_client(stream, &screens, &mut context).await {
                eprintln!("Fehler bei Verbindung: {e}");
            }
        });
    }
}
