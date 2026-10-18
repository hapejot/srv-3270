use std::sync::{Arc, Mutex};

use srv_3270::*;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------

struct LoginScreen {
    state: AppState,
}

impl ScreenTrait for LoginScreen {
    fn data_stream(&self, ctx: &Context) -> DataStream {
        DataStream {
            command: Command::EraseWriteAlternate,
            wcc: Wcc::reset_restore_mdt(),
            width: 80,
            orders: self.orders(ctx, false),
        }
    }

    fn orders(&self, ctx: &Context, updates_only: bool) -> Vec<Order> {
        let name = ctx
            .get("6x21")
            .map(|x| x.as_str().unwrap().to_string())
            .unwrap_or(String::new());
        let dt = ctx
            .get("date")
            .map(|x| x.as_str().unwrap().to_string())
            .unwrap_or(String::new());
        let tm = ctx
            .get("time")
            .map(|x| x.as_str().unwrap().to_string())
            .unwrap_or(String::new());
        let count = self.state.next_counter();

        let mut result = vec![];
        if !updates_only {
            result.extend(
                [
                    Order::SetBufferAddress { row: 1, col: 1 },
                    Order::StartField(FieldAttribute::normal().unprotected().modified()),
                    Order::SetAttribute(Attribute::Highlighting(Highlight::Underscore)),
                    Order::Nulls(4),
                ]
                .into_iter(),
            );
        }
        result.extend(
            [
                Order::SetBufferAddress { row: 1, col: 10 },
                Order::SetAttribute(Attribute::Foreground(Color::NeutralWhite)),
                Order::Text("Interactive Monitoring  ".into()),
                Order::SetBufferAddress { row: 1, col: 60 },
                Order::StartField(FieldAttribute::protected_intensified()),
                Order::Text(format!("{:03}", count)),
                Order::SetBufferAddress { row: 2, col: 60 },
                Order::StartField(FieldAttribute::protected_intensified()),
                Order::Text(dt),
                Order::SetBufferAddress { row: 3, col: 60 },
                Order::StartField(FieldAttribute::protected_intensified()),
                Order::Text(tm),
            ]
            .into_iter(),
        );
        if !updates_only {
            result.extend(
                [
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
                ]
                .into_iter(),
            );
            result.extend(
                [
                    Order::SetBufferAddress { row: 6, col: 5 },
                    Order::StartField(FieldAttribute::normal()),
                    Order::Text("Benutzer:".into()),
                    Order::SetBufferAddress { row: 6, col: 20 },
                    Order::StartField(FieldAttribute::normal().unprotected().modified()),
                    Order::Text(name.clone()),
                    Order::Nulls(20 - name.len()),
                    Order::StartField(FieldAttribute::normal()),
                    Order::Text(format!("user name here")),
                ]
                .into_iter(),
            );
            result.extend(
                [
                    Order::SetBufferAddress { row: 8, col: 5 },
                    Order::StartField(FieldAttribute::normal()),
                    Order::Text("Passwort:".into()),
                    Order::SetBufferAddress { row: 8, col: 20 },
                    Order::StartField(FieldAttribute::unprotected_hidden()),
                    Order::Nulls(20),
                    Order::StartField(FieldAttribute::normal()),
                ]
                .into_iter(),
            );
            result.extend(
                [
                    Order::SetBufferAddress { row: 22, col: 5 },
                    Order::StartField(FieldAttribute::normal()),
                    Order::Text(format!("hint text here")),
                    // Order::SetBufferAddress { row: 6, col: 21 },
                    // Order::InsertCursor,
                ]
                .into_iter(),
            );
            result.extend(
                [
                    Order::SetBufferAddress { row: 6, col: 21 },
                    Order::InsertCursor,
                ]
                .into_iter(),
            );
        }
        result
    }
}

fn _login_screen(width: u16, username_prefill: &str, error: Option<&str>) -> DataStream {
    let hint = error
        .unwrap_or("Enter=Anmelden  Clear/PF3=Abbrechen")
        .to_string();

    DataStream {
        command: Command::EraseWriteAlternate,
        wcc: Wcc::reset_restore_mdt(),
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
            Order::StartField(FieldAttribute::normal()),
            Order::Text("Benutzer:".into()),
            Order::SetBufferAddress { row: 6, col: 20 },
            Order::StartField(FieldAttribute::unprotected_normal()),
            Order::Nulls(20),
            Order::StartField(FieldAttribute::normal()),
            Order::Text(username_prefill.to_string()),
            Order::SetBufferAddress { row: 8, col: 5 },
            Order::StartField(FieldAttribute::normal()),
            Order::Text("Passwort:".into()),
            Order::SetBufferAddress { row: 8, col: 20 },
            Order::StartField(FieldAttribute::unprotected_hidden()),
            Order::Nulls(20),
            Order::StartField(FieldAttribute::normal()),
            Order::SetBufferAddress { row: 22, col: 5 },
            Order::StartField(FieldAttribute::normal()),
            Order::Text(hint),
            // Order::SetBufferAddress { row: 6, col: 21 },
            // Order::InsertCursor,
        ],
    }
}

fn _result_screen(width: u16, username: &str) -> DataStream {
    DataStream {
        command: Command::EraseWrite,
        wcc: Wcc::reset_restore_mdt(),
        width,
        orders: vec![
            Order::SetBufferAddress { row: 2, col: 5 },
            Order::StartField(FieldAttribute::normal()),
            Order::Text(format!("Hallo {}, Login empfangen.", username)),
        ],
    }
}

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<AppStateInner>>,
}

impl AppState {
    fn new() -> Self {
        let inner = Arc::new(Mutex::new(AppStateInner { counter: 0 }));
        Self { inner }
    }

    fn next_counter(&self) -> usize {
        let mut app = self.inner.try_lock().unwrap();
        (*app).counter += 1;
        (*app).counter
    }
}

struct AppStateInner {
    counter: usize,
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

    let state = AppState::new();
    let mut screens = Screens::new();
    screens.add("start", Box::new(LoginScreen { state }));
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
