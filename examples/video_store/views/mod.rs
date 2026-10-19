//! Views der Video-App: Liste mit Paging und Filter, Detail mit Bearbeitung.
//!
//! Der geteilte Zustand ist der OData-Client ([`Odata`]). Die Views selbst
//! kommen aus `srv_3270`; hier liegen nur die fachlichen Helfer.

mod detail;
mod list;

pub use detail::DetailView;
pub use list::ListView;

use serde_json::Value;
use srv_3270::{App, FieldId};

use crate::odata::Odata;

pub const P_ITEMS: &str = "/list/items";

/// Geladene Listenseite (OData-Objekte).
pub fn items(app: &App<Odata>) -> &[Value] {
    app.ctx
        .get(P_ITEMS)
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Eingabewert eines Feldes, falls im Antwortsatz enthalten.
pub fn val<'a>(values: &'a [(FieldId, String)], id: FieldId) -> Option<&'a str> {
    values
        .iter()
        .find(|(f, _)| *f == id)
        .map(|(_, v)| v.as_str())
}

pub fn fmt_duration(secs: Option<f64>) -> String {
    match secs {
        Some(s) => {
            let s = s.round() as u64;
            format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
        }
        None => "-".into(),
    }
}

pub fn fmt_size(bytes: Option<i64>) -> String {
    match bytes {
        Some(b) if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        Some(b) if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        Some(b) => format!("{b} B"),
        None => "-".into(),
    }
}

pub fn fmt_ts(ts: Option<&str>) -> String {
    match ts {
        Some(t) => t.replace('T', " ").chars().take(16).collect(),
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_values() {
        assert_eq!(fmt_duration(Some(3723.4)), "1:02:03");
        assert_eq!(fmt_size(Some(1 << 20)), "1.0 MB");
        assert_eq!(fmt_ts(Some("2024-01-02T10:00:00Z")), "2024-01-02 10:00");
        assert_eq!(fmt_ts(None), "-");
    }
}
