//! Detailansicht eines Videos: Anzeige, Bearbeitung und Speichern.
//!
//! Der Zustand liegt im Datenkontext unter `/detail`. Die Liste bleibt
//! unter `/list` erhalten und wird beim Speichern aktualisiert.

use async_trait::async_trait;
use serde_json::{json, Value};
use srv_3270::{clip, Action, Aid, App, Color, Field, FieldAttribute, FieldId, FieldKind, Screen, View};
use tracing::info;

use super::{fmt_duration, fmt_size, fmt_ts, items, val, P_ITEMS};
use crate::odata::Odata;


const KEYS_DETAIL: &str = "PF3 Zurueck  PF5 Speichern  PF7/PF8 Vor/Zurueck  Enter Uebernehmen";

const P_DETAIL: &str = "/detail";
const P_DETAIL_VIDEO: &str = "/detail/video";
const P_DETAIL_IDX: &str = "/detail/idx";
const P_DETAIL_TITLE: &str = "/detail/title";
const P_DETAIL_REVIEWED: &str = "/detail/reviewed";
const P_DETAIL_VIDEO_TITLE: &str = "/detail/video/title";
const P_DETAIL_VIDEO_REVIEWED: &str = "/detail/video/is_reviewed";
const P_DETAIL_VIDEO_ID: &str = "/detail/video/ID";

const FIELD_TITLE: &str = "title";
const FIELD_REVIEWED: &str = "reviewed";

pub struct DetailView;

/// Bearbeitbare Felder des Details. Anzeige und Pruefung nutzen dieselbe Liste.
fn edit_fields(app: &App<Odata>) -> Vec<Field> {
    let reviewed = app.ctx.get(P_DETAIL_REVIEWED).as_bool().unwrap_or(false);
    vec![
        Field {
            key: FIELD_TITLE,
            label: "Titel",
            len: 56,
            kind: FieldKind::Text,
            value: app
                .ctx
                .get(P_DETAIL_TITLE)
                .as_str()
                .unwrap_or("")
                .to_string(),
        },
        Field {
            key: FIELD_REVIEWED,
            label: "Gesehen (J/N)",
            len: 1,
            kind: FieldKind::Choice(&["J", "N"]),
            value: if reviewed { "J" } else { "N" }.to_string(),
        },
    ]
}

fn has_detail(app: &App<Odata>) -> bool {
    app.ctx.get(P_DETAIL_VIDEO).is_object()
}

fn detail_idx(app: &App<Odata>) -> Option<usize> {
    app.ctx.get(P_DETAIL_IDX).as_u64().map(|i| i as usize)
}

fn set_detail(app: &mut App<Odata>, idx: usize, video: Value) {
    let title = video["title"].as_str().unwrap_or("").to_string();
    let reviewed = video["is_reviewed"].as_bool().unwrap_or(false);
    app.ctx.set(P_DETAIL_IDX, json!(idx));
    app.ctx.set(P_DETAIL_TITLE, json!(title));
    app.ctx.set(P_DETAIL_REVIEWED, json!(reviewed));
    app.ctx.set(P_DETAIL_VIDEO, video);
}

fn detail_dirty(app: &App<Odata>) -> bool {
    let saved_title = app.ctx.get(P_DETAIL_VIDEO_TITLE).as_str().unwrap_or("");
    let saved_reviewed = app
        .ctx
        .get(P_DETAIL_VIDEO_REVIEWED)
        .as_bool()
        .unwrap_or(false);
    app.ctx.get(P_DETAIL_TITLE).as_str().unwrap_or("") != saved_title
        || app.ctx.get(P_DETAIL_REVIEWED).as_bool().unwrap_or(false) != saved_reviewed
}

/// Lädt das Video zum Listeneintrag `idx` vom Service und macht es zum Detail.
async fn load_detail(app: &mut App<Odata>, idx: usize) -> Result<(), String> {
    let id = items(app)
        .get(idx)
        .and_then(|v| v["ID"].as_str())
        .map(str::to_owned)
        .ok_or("Fehler beim Laden des Details: keine Zeile unter diesem Eintrag")?;
    let video = app
        .shared
        .get(&id)
        .await
        .map_err(|e| format!("Fehler beim Laden des Details: {e}"))?;
    set_detail(app, idx, video);
    Ok(())
}

async fn step_detail(app: &mut App<Odata>, delta: isize) {
    let Some(cur) = detail_idx(app) else {
        return;
    };
    let next = cur as isize + delta;
    if next < 0 || next as usize >= items(app).len() {
        app.msg = "Kein weiterer Eintrag".into();
        return;
    }
    if let Err(e) = load_detail(app, next as usize).await {
        app.msg = e;
    }
}

fn apply_inputs(app: &mut App<Odata>, values: &[(FieldId, String)]) {
    if !has_detail(app) {
        return;
    }
    let mut checked = Vec::new();
    for f in edit_fields(app) {
        let Some(raw) = val(values, FieldId::Named(f.key)) else {
            continue;
        };
        match f.check(raw) {
            Ok(v) => checked.push((f.key, v)),
            Err(e) => {
                app.msg = e;
                return;
            }
        }
    }
    for (key, v) in checked {
        match key {
            FIELD_TITLE => app.ctx.set(P_DETAIL_TITLE, json!(clip(&v, 512))),
            FIELD_REVIEWED => app.ctx.set(P_DETAIL_REVIEWED, json!(v == "J")),
            _ => {}
        }
    }
    app.msg = if detail_dirty(app) {
        "Uebernommen - PF5 speichert".into()
    } else {
        "Keine Aenderungen".into()
    };
}

async fn save(app: &mut App<Odata>) {
    if !has_detail(app) {
        return;
    }
    if !detail_dirty(app) {
        app.msg = "Keine Aenderungen".into();
        return;
    }
    let id = app
        .ctx
        .get(P_DETAIL_VIDEO_ID)
        .as_str()
        .unwrap_or("")
        .to_string();
    let title = app
        .ctx
        .get(P_DETAIL_TITLE)
        .as_str()
        .unwrap_or("")
        .to_string();
    let reviewed = app.ctx.get(P_DETAIL_REVIEWED).as_bool().unwrap_or(false);
    let idx = detail_idx(app).unwrap_or(0);
    if let Err(e) = app.shared.patch(&id, &title, reviewed).await {
        app.msg = format!("Speichern fehlgeschlagen: {e}");
        return;
    }
    // Neu laden, damit Detail und Liste den gespeicherten Stand zeigen.
    match app.shared.get(&id).await {
        Ok(video) => {
            if let Some(item) = app.ctx.get_mut(&format!("{P_ITEMS}/{idx}")) {
                *item = video.clone();
            }
            set_detail(app, idx, video);
            app.msg = "Gespeichert".into();
        }
        Err(e) => app.msg = format!("Gespeichert, aber Neuladen fehlgeschlagen: {e}"),
    }
}

fn tag_names(v: &Value) -> Vec<String> {
    v["_VideoTags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["tag"]["name"].as_str().map(str::to_owned))
        .collect()
}

impl DetailView {
    fn render_detail(app: &App<Odata>) -> Screen {
        let (rows, cols) = (app.rows, app.cols);
        let mut s = Screen::new(rows, cols);
        let v = app.ctx.get(P_DETAIL_VIDEO);
        let idx = detail_idx(app).unwrap_or(0);
        let pos = format!("{}/{}", idx + 1, items(app).len());
        s.text(
            0,
            0,
            &format!("VIDEO-STORE  Detail {pos}"),
            FieldAttribute::protected_intensified(),
            Some(Color::Turquoise),
        );
        let marker = if detail_dirty(app) {
            "  *geaendert*"
        } else {
            ""
        };
        s.text(
            1,
            0,
            &format!("{}{marker}", v["name"].as_str().unwrap_or_default()),
            FieldAttribute::normal(),
            None,
        );

        let fa = FieldAttribute::normal();
        s.form(3, &edit_fields(app));

        let res = match (v["width"].as_i64(), v["height"].as_i64()) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "-".into(),
        };
        let bitrate = v["bitrate"]
            .as_i64()
            .map(|b| format!("{} kbit/s", b / 1000))
            .unwrap_or_else(|| "-".into());
        let rows_kv = [
            (
                "Dateipfad",
                v["file_path"].as_str().unwrap_or("").to_string(),
            ),
            ("Dauer", fmt_duration(v["duration_seconds"].as_f64())),
            ("Aufloesung", res),
            ("Codec", v["codec"].as_str().unwrap_or("-").to_string()),
            ("Bitrate", bitrate),
            ("Groesse", fmt_size(v["file_size"].as_i64())),
            (
                "Vorschaubilder",
                v["thumbnail_count"]
                    .as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "-".into()),
            ),
            ("Erfasst", fmt_ts(v["created_at"].as_str())),
            ("Aktualisiert", fmt_ts(v["updated_at"].as_str())),
            ("Zuletzt gesehen", fmt_ts(v["last_seen"].as_str())),
        ];
        for (i, (label, value)) in rows_kv.iter().enumerate() {
            let row = 6 + i as u16;
            s.text(row, 2, label, fa, None);
            s.text(row, 20, value, fa, None);
        }

        info!(?v);

        let mut tags = tag_names(v);
        tags.sort();
        let mut row = 17u16;
        s.text(row, 2, "Tags", fa, None);
        if tags.is_empty() {
                s.text(row, 20, "-", fa, None);
        } else {
            let mut col = 0;
            for tag in tags.iter() {
                s.text(row, 20 + col * 20, tag, fa, None);
                col += 1;
                if col > 2 {
                    col = 0;
                    row += 1;
                }
            }
        }

        s.text(
            rows - 2,
            0,
            &app.msg,
            FieldAttribute::protected_intensified(),
            None,
        );
        s.text(rows - 1, 0, KEYS_DETAIL, FieldAttribute::normal(), None);
        s.focus(FieldId::Named(FIELD_TITLE));
        s
    }
}

#[async_trait]
impl View<Odata> for DetailView {
    async fn enter(&mut self, app: &mut App<Odata>, params: Value) -> Result<(), String> {
        let idx = params["idx"].as_u64().ok_or("Detail ohne Eintragsnummer")? as usize;
        load_detail(app, idx).await
    }

    fn render(&self, app: &App<Odata>) -> Screen {
        Self::render_detail(app)
    }

    async fn handle(&mut self, app: &mut App<Odata>, aid: Aid, values: &[(FieldId, String)]) -> Action {
        match aid {
            Aid::Pf(3) | Aid::Clear => {
                app.ctx.set(P_DETAIL, Value::Null);
                return Action::Back;
            }
            Aid::Pf(7) => step_detail(app, -1).await,
            Aid::Pf(8) => step_detail(app, 1).await,
            Aid::Pf(5) => save(app).await,
            Aid::Enter => apply_inputs(app, values),
            _ => app.msg = "Taste hier nicht belegt".into(),
        }
        Action::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use srv_3270::Context;

    fn app() -> App<Odata> {
        App {
            shared: Odata::new("http://localhost:1/Service".into(), "u".into()).unwrap(),
            ctx: Context::default(),
            msg: String::new(),
            rows: 24,
            cols: 80,
        }
    }

    #[test]
    fn detail_is_dirty_only_after_change() {
        let mut a = app();
        let video = json!({ "ID": "a", "title": "Film", "is_reviewed": false });
        set_detail(&mut a, 0, video);
        assert!(!detail_dirty(&a));

        a.ctx.set(P_DETAIL_TITLE, json!("Neu"));
        assert!(detail_dirty(&a));

        a.ctx.set(P_DETAIL_TITLE, json!("Film"));
        a.ctx.set(P_DETAIL_REVIEWED, json!(true));
        assert!(detail_dirty(&a));
    }

    #[test]
    fn tag_names_reads_nested_tags() {
        let v = json!({ "_VideoTags": [{ "tag": { "name": "Doku" } }, { "tag": null }] });
        assert_eq!(tag_names(&v), vec!["Doku".to_string()]);
    }

    #[test]
    fn invalid_reviewed_value_is_rejected_and_keeps_state() {
        let mut a = app();
        set_detail(
            &mut a,
            0,
            json!({ "ID": "a", "title": "Film", "is_reviewed": true }),
        );
        let values = [(FieldId::Named(FIELD_REVIEWED), "X".to_string())];
        apply_inputs(&mut a, &values);
        assert!(a.msg.contains("J, N"));
        assert_eq!(a.ctx.get(P_DETAIL_REVIEWED).as_bool(), Some(true));
    }

    #[test]
    fn valid_reviewed_value_updates_state() {
        let mut a = app();
        set_detail(
            &mut a,
            0,
            json!({ "ID": "a", "title": "Film", "is_reviewed": true }),
        );
        let values = [(FieldId::Named(FIELD_REVIEWED), "n".to_string())];
        apply_inputs(&mut a, &values);
        assert_eq!(a.ctx.get(P_DETAIL_REVIEWED).as_bool(), Some(false));
        assert!(detail_dirty(&a));
    }
}
