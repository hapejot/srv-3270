//! Listenansicht: Filter, Paging und Auswahl eines Videos.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{P_ITEMS, fmt_duration, fmt_size, items, val};
use crate::odata::{Filter, Odata};
use srv_3270::{Action, Aid, App, Color, FieldAttribute, FieldId, Screen, View, clip};

const LIST_TOP: u16 = 3;
const KEYS_LIST: &str =
    "PF3 Ende  PF5 Neu laden  PF7 Zurueck  PF8 Weiter  Enter=Ausfuehren/Oeffnen";
const HELP: &str =
    "Befehle: /text  T tag  N ungesehen  X alle Filter  PF5 neu laden  S-Feld+Enter oeffnet";

const P_COUNT: &str = "/list/count";
const P_PAGE: &str = "/list/page";

#[derive(Default)]
pub struct ListView {
    filter: Filter,
}

fn page_size(app: &App<Odata>) -> usize {
    (app.rows.saturating_sub(LIST_TOP + 4)).max(1) as usize
}

fn count(app: &App<Odata>) -> Option<i64> {
    app.ctx.get(P_COUNT).as_i64()
}

fn page(app: &App<Odata>) -> usize {
    app.ctx.get(P_PAGE).as_u64().unwrap_or(0) as usize
}

fn set_page(app: &mut App<Odata>, page: usize) {
    app.ctx.set(P_PAGE, json!(page));
}

fn pages(app: &App<Odata>) -> usize {
    match count(app) {
        Some(c) if c > 0 => (c as usize).div_ceil(page_size(app)),
        _ => 1,
    }
}

fn has_next(app: &App<Odata>) -> bool {
    let skip = page(app) * page_size(app);
    match count(app) {
        Some(c) => skip + page_size(app) < c as usize,
        None => items(app).len() == page_size(app),
    }
}

fn display_title(v: &Value) -> String {
    match v["title"].as_str().filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => v["name"].as_str().unwrap_or("").to_string(),
    }
}

fn list_line(v: &Value) -> String {
    let res = match (v["width"].as_i64(), v["height"].as_i64()) {
        (Some(w), Some(h)) => format!("{w}x{h}"),
        _ => "-".into(),
    };
    let seen = match v["is_reviewed"].as_bool() {
        Some(true) => "J",
        _ => "-",
    };
    columns(
        &clip(&display_title(v), 34),
        &fmt_duration(v["duration_seconds"].as_f64()),
        &res,
        &clip(v["codec"].as_str().unwrap_or("-"), 6),
        &fmt_size(v["file_size"].as_i64()),
        seen,
    )
}

fn columns(
    titel: &str,
    dauer: &str,
    aufl: &str,
    codec: &str,
    groesse: &str,
    gesehen: &str,
) -> String {
    format!("{titel:<34} {dauer:>8} {aufl:<10} {codec:<6} {groesse:>9} {gesehen}")
}

impl ListView {
    async fn load(&self, app: &mut App<Odata>) {
        let size = page_size(app);
        let skip = page(app) * size;
        match app.shared.list(&self.filter, skip, size).await {
            Ok((items, total)) => {
                app.ctx.set(P_ITEMS, Value::Array(items));
                app.ctx.set(P_COUNT, json!(total));
            }
            Err(e) => app.msg = format!("Fehler beim Laden: {e}"),
        }
    }

    async fn run_command(&mut self, app: &mut App<Odata>, cmd: &str) {
        if cmd.is_empty() {
            self.load(app).await;
            app.msg = "Liste aktualisiert".into();
            return;
        }
        let (head, rest) = if let Some(r) = cmd.strip_prefix('/') {
            ("/", r.trim())
        } else {
            match cmd.find(' ') {
                Some(i) => (&cmd[..i], cmd[i + 1..].trim()),
                None => (cmd, ""),
            }
        };
        let arg = if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        };
        match head.to_uppercase().as_str() {
            "/" | "F" => self.filter.text = arg,
            "T" => self.filter.tag = arg,
            "N" => self.filter.only_unreviewed = true,
            "X" => self.filter = Filter::default(),
            "?" | "H" => {
                app.msg = HELP.into();
                return;
            }
            _ => {
                app.msg = format!("Unbekannter Befehl: {cmd}");
                return;
            }
        }
        set_page(app, 0);
        self.load(app).await;
        if app.msg.is_empty() {
            let shown = count(app).unwrap_or(items(app).len() as i64);
            app.msg = format!("{shown} Treffer");
        }
    }
}

#[async_trait]
impl View<Odata> for ListView {
    async fn enter(&mut self, app: &mut App<Odata>, _params: Value) -> Result<(), String> {
        self.load(app).await;
        Ok(())
    }

    fn render(&self, app: &App<Odata>) -> Screen {
        let (rows, cols) = (app.rows, app.cols);
        let mut s = Screen::new(rows, cols);
        let total = count(app)
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());
        s.text(
            0,
            0,
            &format!(
                "VIDEO-STORE  Videoliste   Seite {}/{}   Treffer {}",
                page(app) + 1,
                pages(app),
                total
            ),
            FieldAttribute::protected_intensified(),
            Some(Color::Blue),
        );
        s.text(
            1,
            0,
            &format!("Filter: {}", self.filter.describe()),
            FieldAttribute::normal(),
            None,
        );
        s.text(
            2,
            2,
            &columns("Titel", "Dauer", "Aufloes.", "Codec", "Groesse", "G"),
            FieldAttribute::protected_intensified(),
            Some(Color::White),
        );
        for (i, v) in items(app).iter().enumerate().take(page_size(app)) {
            let row = LIST_TOP + i as u16;
            s.input(FieldId::Select(i), row, 0, 1, "");
            s.text(
                row,
                2,
                &list_line(v),
                FieldAttribute::normal(),
                Some(Color::Green),
            );
        }
        let r_cmd = rows - 3;
        s.text(
            r_cmd,
            0,
            "Befehl ==>",
            FieldAttribute::normal(),
            Some(Color::Blue),
        );
        s.input(FieldId::Command, r_cmd, 11, cols - 12, "");
        s.focus(FieldId::Command);
        s.text(
            rows - 2,
            0,
            &app.msg,
            FieldAttribute::protected_intensified(),
            Some(Color::Yellow),
        );
        s.text(
            rows - 1,
            0,
            KEYS_LIST,
            FieldAttribute::normal(),
            Some(Color::Blue),
        );
        s
    }

    async fn handle(
        &mut self,
        app: &mut App<Odata>,
        aid: Aid,
        values: &[(FieldId, String)],
    ) -> Action {
        match aid {
            Aid::Pf(3) | Aid::Clear => return Action::Exit,
            Aid::Pf(5) => {
                self.load(app).await;
                app.msg = "Liste neu geladen".into();
            }
            Aid::Pf(7) => {
                let current = page(app);
                if current > 0 {
                    set_page(app, current - 1);
                    self.load(app).await;
                }
            }
            Aid::Pf(8) => {
                if has_next(app) {
                    let current = page(app);
                    set_page(app, current + 1);
                    self.load(app).await;
                }
            }
            Aid::Enter => {
                let selected = values.iter().find_map(|(id, v)| match id {
                    FieldId::Select(i) if !v.is_empty() => Some(*i),
                    _ => None,
                });
                match selected {
                    Some(idx) => {
                        return Action::Open {
                            view: "detail",
                            params: json!({ "idx": idx }),
                        };
                    }
                    None => {
                        let cmd = val(values, FieldId::Command)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        self.run_command(app, &cmd).await;
                    }
                }
            }
            _ => app.msg = "Taste hier nicht belegt".into(),
        }
        Action::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_title_falls_back_to_name() {
        assert_eq!(display_title(&json!({ "title": "T", "name": "N" })), "T");
        assert_eq!(display_title(&json!({ "title": "", "name": "N" })), "N");
        assert_eq!(display_title(&json!({ "name": "N" })), "N");
    }

    #[test]
    fn list_line_reads_json_fields() {
        let line = list_line(&json!({
            "title": "Film",
            "duration_seconds": 90.0,
            "width": 1920,
            "height": 1080,
            "codec": "h264",
            "file_size": 1024,
            "is_reviewed": true,
        }));
        assert!(line.starts_with("Film"));
        assert!(line.contains("1920x1080"));
        assert!(line.contains("h264"));
        assert!(line.trim_end().ends_with('J'));
    }
}
