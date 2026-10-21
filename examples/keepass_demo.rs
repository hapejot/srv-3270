//! KeePass-Demo fuer srv-3270: Anmeldung -> Eintraege (Auswahl + Befehlszeile) -> Details.
//!
//! Vorbereitung (einmalig): `cargo run --example make_demo_db`
//! Start:  `cargo run --example keepass_demo -- examples/data/demo.kdbx`
//!         (Port per `--port`, Standard 3271; Master-Passwort: demo1234)
//! Verbinden: `x3270 localhost:3271` oder `c3270 localhost:3271`
//!
//! Tasten:
//!   Anmeldung: Enter=Anmelden, PF3/Clear=Ende
//!   Liste:     Zeichen in Spalte S + Enter = Eintrag oeffnen
//!              Befehl "/text" = Filter setzen, "X" = Filter loeschen
//!              PF7/PF8 = Seite zurueck/vor, PF3 = Abmelden, Clear = Ende
//!   Details:   PF2 = Passwort ein/aus, Enter/PF3 = zurueck, Clear = Ende

use std::path::PathBuf;

use anyhow::{Result, bail};
use async_trait::async_trait;
use clap::Parser;
use keepass::{Database, DatabaseKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use srv_3270::{
    Action, Aid, App, Color, Field, FieldAttribute, FieldId, FieldKind, Patch, Screen, Server,
    View, clip, data_marker,
};
use tracing::info;
use tracing_subscriber::EnvFilter;

const MAX_ATTEMPTS: u32 = 3;
const LIST_TOP: u16 = 4;
const P_ENTRIES: &str = "/entries";
const P_FILTER: &str = "/filter";

#[derive(Debug, Parser)]
struct Args {
    /// Pfad zur KeePass-Datenbank (.kdbx)
    database: PathBuf,
    #[clap(long, default_value_t = 3271)]
    port: u16,
}

/// Vom Server an jede Sitzung geklonter Zustand.
#[derive(Clone)]
struct Shared {
    database: PathBuf,
}

/// Ein Eintrag aus der Datenbank. Liegt pro Sitzung im Datenkontext.
#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    title: String,
    user: String,
    url: String,
    password: String,
}

// ---------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------

/// Oeffnet die Datenbank mit dem Passwort und liefert alle Eintraege.
fn load_entries(path: &PathBuf, password: &str) -> Result<Vec<Entry>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Datenbank nicht lesbar: {e}"))?;
    let db = Database::open(&mut file, DatabaseKey::new().with_password(password))
        .map_err(|_| "Falsches Passwort".to_string())?;
    Ok(db
        .iter_all_entries()
        .map(|e| Entry {
            title: e.get_title().unwrap_or("").to_string(),
            user: e.get_username().unwrap_or("").to_string(),
            url: e.get_url().unwrap_or("").to_string(),
            password: e.get_password().unwrap_or("").to_string(),
        })
        .collect())
}

fn entries(app: &App<Shared>) -> Vec<Entry> {
    serde_json::from_value(app.ctx.get(P_ENTRIES).clone()).unwrap_or_default()
}

fn filter(app: &App<Shared>) -> String {
    app.ctx.get(P_FILTER).as_str().unwrap_or("").to_string()
}

/// Indizes der Eintraege, die zum Filter passen (Titel oder Benutzer, ohne Gross/Klein).
fn matches(all: &[Entry], f: &str) -> Vec<usize> {
    let f = f.to_lowercase();
    all.iter()
        .enumerate()
        .filter(|(_, e)| {
            f.is_empty()
                || e.title.to_lowercase().contains(&f)
                || e.user.to_lowercase().contains(&f)
        })
        .map(|(i, _)| i)
        .collect()
}

/// Eingabe eines Feldes, falls im Antwortsatz enthalten.
fn value(values: &[(FieldId, String)], id: FieldId) -> Option<&str> {
    values
        .iter()
        .find(|(f, _)| *f == id)
        .map(|(_, v)| v.as_str())
}

/// Titelzeile und Fusszeile mit Meldung und Tastenhinweis.
fn frame(s: &mut Screen, title: &str, msg: &str, keys: &str) {
    s.text(
        0,
        0,
        title,
        FieldAttribute::protected_intensified(),
        Some(Color::Blue),
    );
    s.text(
        s.rows - 2,
        0,
        msg,
        FieldAttribute::protected_intensified(),
        Some(Color::Yellow),
    );
    s.text(
        s.rows - 1,
        0,
        keys,
        FieldAttribute::normal(),
        Some(Color::Blue),
    );
}

// ---------------------------------------------------------------------
// Anmeldung
// ---------------------------------------------------------------------

#[derive(Default)]
struct Login {
    attempts: u32,
}

#[async_trait]
impl View<Shared> for Login {
    fn render(&self, app: &App<Shared>) -> Screen {
        let mut s = Screen::new(app.rows, app.cols);
        frame(
            &mut s,
            "KeePass-Demo - Anmeldung",
            &app.msg,
            "Enter=Anmelden  PF3/Clear=Ende",
        );
        let fields = [Field {
            key: "pw",
            label: "Master-Passwort:",
            len: 24,
            kind: FieldKind::Secret,
            value: String::new(),
        }];
        s.form(5, &fields);
        s.focus(FieldId::Named("pw"));
        s
    }

    async fn handle(
        &mut self,
        app: &mut App<Shared>,
        aid: Aid,
        values: &[(FieldId, String)],
    ) -> Action {
        match aid {
            Aid::Pf(3) | Aid::Clear => Action::Exit,
            Aid::Enter => {
                let pw = value(values, FieldId::Named("pw"))
                    .unwrap_or("")
                    .to_string();
                if pw.is_empty() {
                    app.msg = "Bitte Passwort eingeben.".into();
                    return Action::Stay;
                }
                match load_entries(&app.shared.database, &pw) {
                    Ok(list) => {
                        info!("Anmeldung erfolgreich, {} Eintraege", list.len());
                        app.ctx.set(P_ENTRIES, serde_json::to_value(&list).unwrap());
                        app.ctx.set(P_FILTER, json!(""));
                        app.msg.clear();
                        Action::Open {
                            view: "list",
                            params: Value::Null,
                        }
                    }
                    Err(msg) => {
                        self.attempts += 1;
                        info!(
                            "Anmeldung fehlgeschlagen ({}/{MAX_ATTEMPTS}): {msg}",
                            self.attempts
                        );
                        if self.attempts >= MAX_ATTEMPTS {
                            return Action::Exit;
                        }
                        app.msg = format!("{msg} ({}/{MAX_ATTEMPTS})", self.attempts);
                        Action::Stay
                    }
                }
            }
            _ => {
                app.msg = "Taste hier nicht belegt".into();
                Action::Stay
            }
        }
    }
}

// ---------------------------------------------------------------------
// Liste mit Auswahl, Filter und Paging
// ---------------------------------------------------------------------

#[derive(Default)]
struct List {
    page: usize,
}

impl List {
    fn page_size(app: &App<Shared>) -> usize {
        (app.rows.saturating_sub(LIST_TOP + 4)).max(1) as usize
    }
}

/// Uhrzeit rechts in der Titelzeile: Feldattribut bei `CLOCK_COL`, Daten ab
/// `CLOCK_COL + 1`. Teil-Updates muessen daher bei `CLOCK_COL + 1` beginnen.
const CLOCK_LEN: usize = 8; // "HH:MM:SS"

fn clock_col(cols: u16) -> u16 {
    cols.saturating_sub(CLOCK_LEN as u16 + 2)
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

#[async_trait]
impl View<Shared> for List {
    /// Uhrzeit jede Sekunde per Server Push, ohne Tastendruck.
    fn refresh_interval(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(1))
    }

    fn refresh(&self, app: &App<Shared>) -> Option<Patch> {
        let mut p = Patch::new();
        p.text(0, clock_col(app.cols) + 1, &now_hms(), CLOCK_LEN);
        Some(p)
    }

    fn render(&self, app: &App<Shared>) -> Screen {
        let all = entries(app);
        let f = filter(app);
        let hits = matches(&all, &f);
        let size = Self::page_size(app);
        let pages = hits.len().div_ceil(size).max(1);

        let mut s = Screen::new(app.rows, app.cols);
        frame(
            &mut s,
            &format!(
                "KeePass-Demo  Eintraege  Seite {}/{}  Treffer {}",
                self.page + 1,
                pages,
                hits.len()
            ),
            &app.msg,
            "S+Enter=Oeffnen  /Text=Filter  X=Filter weg  PF7/8=Seite  PF3=Abmelden",
        );
        s.text(
            0,
            clock_col(app.cols),
            &now_hms(),
            FieldAttribute::protected_intensified(),
            Some(Color::Blue),
        );
        s.text(
            1,
            0,
            &format!("Filter: {}", if f.is_empty() { "(alle)" } else { &f }),
            FieldAttribute::normal(),
            None,
        );
        s.text(
            2,
            0,
            &format!("S {:<30} {}", "Titel", "Benutzer"),
            FieldAttribute::protected_intensified(),
            Some(Color::White),
        );
        for (i, idx) in hits.iter().skip(self.page * size).take(size).enumerate() {
            let row = LIST_TOP + i as u16;
            let e = &all[*idx];
            // Auswahlfeld in Spalte 0; die Id ist der Eintragsindex.
            s.input(FieldId::Select(*idx), row, 0, 1, "");
            s.text(
                row,
                2,
                &format!("{:<30} {}", clip(&e.title, 30), clip(&e.user, 40)),
                FieldAttribute::normal(),
                Some(Color::Green),
            );
        }
        // Top/Bottom of Data: Top auf der ersten Seite, Bottom nach dem letzten Eintrag.
        if self.page == 0 {
            s.text(
                LIST_TOP - 1,
                0,
                &data_marker("Top of Data", app.cols as usize),
                FieldAttribute::normal(),
                Some(Color::Blue),
            );
        }
        let shown = hits.iter().skip(self.page * size).take(size).count();
        let bottom_row = LIST_TOP + shown as u16;
        if self.page + 1 >= pages && bottom_row < app.rows - 3 {
            s.text(
                bottom_row,
                0,
                &data_marker("Bottom of Data", app.cols as usize),
                FieldAttribute::normal(),
                Some(Color::Blue),
            );
        }
        let r_cmd = app.rows - 3;
        s.text(
            r_cmd,
            0,
            "Befehl ==>",
            FieldAttribute::normal(),
            Some(Color::Blue),
        );
        s.input(FieldId::Command, r_cmd, 11, app.cols - 12, "");
        s.focus(FieldId::Command);
        s
    }

    async fn handle(
        &mut self,
        app: &mut App<Shared>,
        aid: Aid,
        values: &[(FieldId, String)],
    ) -> Action {
        match aid {
            Aid::Clear => return Action::Exit,
            Aid::Pf(3) => {
                app.ctx.set(P_ENTRIES, json!([]));
                return Action::Back;
            }
            Aid::Pf(7) => self.page = self.page.saturating_sub(1),
            Aid::Pf(8) => {
                let pages = matches(&entries(app), &filter(app))
                    .len()
                    .div_ceil(Self::page_size(app));
                if self.page + 1 < pages {
                    self.page += 1;
                }
            }
            Aid::Enter => {
                // Eine markierte Auswahl hat Vorrang vor der Befehlszeile.
                let selected = values.iter().find_map(|(id, v)| match id {
                    FieldId::Select(i) if !v.is_empty() => Some(*i),
                    _ => None,
                });
                if let Some(idx) = selected {
                    return Action::Open {
                        view: "detail",
                        params: json!({ "idx": idx }),
                    };
                }
                let cmd = value(values, FieldId::Command)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.command(app, &cmd);
            }
            _ => app.msg = "Taste hier nicht belegt".into(),
        }
        Action::Stay
    }
}

impl List {
    fn command(&mut self, app: &mut App<Shared>, cmd: &str) {
        app.msg.clear();
        if cmd.is_empty() {
            return;
        }
        if let Some(text) = cmd.strip_prefix('/') {
            app.ctx.set(P_FILTER, json!(text.trim()));
            self.page = 0;
        } else if cmd.eq_ignore_ascii_case("x") {
            app.ctx.set(P_FILTER, json!(""));
            self.page = 0;
        } else if cmd == "?" {
            app.msg = "Befehle: /Text Filter, X Filter weg, S+Enter oeffnen".into();
        } else {
            app.msg = format!("Unbekannter Befehl: {cmd}");
        }
    }
}

// ---------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------

#[derive(Default)]
struct Detail {
    idx: usize,
    reveal: bool,
}

#[async_trait]
impl View<Shared> for Detail {
    async fn enter(&mut self, app: &mut App<Shared>, params: Value) -> Result<(), String> {
        let idx = params["idx"].as_u64().ok_or("Detail ohne Eintragsnummer")? as usize;
        if idx >= entries(app).len() {
            return Err("Eintrag nicht gefunden".into());
        }
        self.idx = idx;
        Ok(())
    }

    fn render(&self, app: &App<Shared>) -> Screen {
        let mut s = Screen::new(app.rows, app.cols);
        frame(
            &mut s,
            "KeePass-Demo  Details",
            &app.msg,
            "PF2=Passwort ein/aus  Enter/PF3=Zurueck  Clear=Ende",
        );
        let all = entries(app);
        let Some(e) = all.get(self.idx) else {
            return s;
        };
        let pw = if self.reveal {
            e.password.clone()
        } else {
            "********".to_string()
        };
        let rows = [
            ("Titel:", e.title.as_str()),
            ("Benutzer:", e.user.as_str()),
            ("URL:", e.url.as_str()),
            ("Passwort:", pw.as_str()),
        ];
        for (i, (label, val)) in rows.iter().enumerate() {
            let row = 3 + 2 * i as u16;
            s.text(row, 2, label, FieldAttribute::normal(), Some(Color::Blue));
            s.text(
                row,
                20,
                &clip(val, 50),
                FieldAttribute::normal(),
                Some(Color::Green),
            );
        }
        s
    }

    async fn handle(
        &mut self,
        app: &mut App<Shared>,
        aid: Aid,
        _values: &[(FieldId, String)],
    ) -> Action {
        match aid {
            Aid::Clear => Action::Exit,
            Aid::Pf(2) => {
                self.reveal = !self.reveal;
                app.msg.clear();
                Action::Stay
            }
            Aid::Enter | Aid::Pf(3) => Action::Back,
            _ => {
                app.msg = "Taste hier nicht belegt".into();
                Action::Stay
            }
        }
    }
}

// ---------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    if !args.database.is_file() {
        bail!("Datenbank nicht gefunden: {}", args.database.display());
    }

    let shared = Shared {
        database: args.database,
    };
    Server::new(shared, "login")
        .view("login", Login::default)
        .view("list", List::default)
        .view("detail", Detail::default)
        .serve(("0.0.0.0", args.port))
        .await
}
