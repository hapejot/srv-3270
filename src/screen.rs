//! Bildschirm-Aufbau: geschuetzte Texte und Eingabefelder, uebersetzt in
//! einen 3270-Datenstrom. Eingabefelder sind benannt ([`FieldId`]), damit die
//! Antwort des Terminals einem Feld zugeordnet werden kann.

use crate::datastream::{Command, DataStream, FieldAttribute, Order, Wcc};
use crate::{Attribute, Color, Highlight};

/// Identitaet eines Eingabefeldes auf dem Bildschirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    /// Auswahlfeld einer Tabellenzeile (Index der Zeile, z. B. Trefferliste).
    /// Der Benutzer tippt ein Zeichen in die Spalte links neben der Zeile.
    Select(usize),
    /// Befehlszeile (`Befehl ==>`).
    Command,
    /// Benanntes Formularfeld.
    Named(&'static str),
}

/// Art der Eingabe eines Formularfeldes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Beliebiger Text bis zur Feldlaenge.
    Text,
    /// Ganze Zahl mit optionalem Vorzeichen. Das Terminal laesst nur Ziffern zu.
    Number,
    /// Eingabe ohne Anzeige (Passwort). Das Terminal zeigt keine Zeichen.
    Secret,
    /// Einer der angegebenen Werte, Gross- und Kleinschreibung egal.
    Choice(&'static [&'static str]),
}

/// Ein Formularfeld: Beschriftung, Laenge, Art und aktueller Wert.
#[derive(Debug, Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub len: u16,
    pub kind: FieldKind,
    pub value: String,
}

impl Field {
    /// Prueft eine Eingabe und liefert den Wert in kanonischer Form.
    /// Leere Zahleneingaben sind erlaubt und bedeuten "kein Wert".
    pub fn check(&self, raw: &str) -> Result<String, String> {
        let raw = raw.trim();
        match self.kind {
            FieldKind::Text | FieldKind::Secret => Ok(raw.to_string()),
            FieldKind::Number => {
                if raw.is_empty() || raw.parse::<i64>().is_ok() {
                    Ok(raw.to_string())
                } else {
                    Err(format!("{}: Zahl erwartet", self.label))
                }
            }
            FieldKind::Choice(options) => options
                .iter()
                .find(|o| o.eq_ignore_ascii_case(raw))
                .map(|o| o.to_string())
                .ok_or_else(|| format!("{}: erlaubt {}", self.label, options.join(", "))),
        }
    }
}

/// Ein Eingabefeld im fertigen Screen. `pos` ist die lineare Position des
/// Feld-Attributbytes (row * cols + col); die Daten beginnen bei `pos + 1`.
#[derive(Debug, Clone)]
pub struct InputField {
    pub id: FieldId,
    pub pos: usize,
    pub row: u16,
    pub col: u16,
    pub len: u16,
}

/// Spalte, ab der Formularbeschriftungen und Eingaben stehen.
pub const LABEL_COL: u16 = 2;
pub const INPUT_COL: u16 = 20;

pub struct Screen {
    pub rows: u16,
    pub cols: u16,
    orders: Vec<Order>,
    pub inputs: Vec<InputField>,
    cursor: Option<(u16, u16)>,
}

/// Markierungszeile wie `*** Top of Data ***`, die ueber die gesamte
/// Breite `width` mit Sternchen gefuellt wird. Ueberschuessige Sterne
/// stehen rechts.
pub fn data_marker(label: &str, width: usize) -> String {
    let body = format!(" {label} ");
    let len = body.chars().count();
    if width <= len + 2 {
        return clip(&body, width);
    }
    let stars = width - len;
    let left = stars / 2;
    let right = stars - left;
    format!("{}{}{}", "*".repeat(left), body, "*".repeat(right))
}

/// Kuerzt einen Text auf maximal `max` Zeichen.
pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn pad(s: &str, len: usize) -> String {
    let mut t = clip(s, len);
    while t.chars().count() < len {
        t.push(' ');
    }
    t
}

/// Teilaktualisierung eines bestehenden Bildschirms (Server Push). Sendet
/// `Write` ohne Erase, ohne MDT-Reset und ohne Cursor: Eingabefelder, Cursor
/// und bereits getippte Eingaben bleiben erhalten. Die Texte muessen innerhalb
/// geschuetzter Bereiche liegen, sonst wuerden Feld-Attribute ueberschrieben.
#[derive(Debug, Default)]
pub struct Patch {
    items: Vec<(u16, u16, String)>,
}

impl Patch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schreibt `s` auf genau `len` Zeichen ab (row, col). Kuerzen bzw.
    /// Auffuellen mit Leerzeichen, damit alte Zeichen ueberschrieben werden.
    pub fn text(&mut self, row: u16, col: u16, s: &str, len: usize) {
        self.items.push((row, col, pad(s, len)));
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn to_stream(&self, cols: u16) -> DataStream {
        let mut orders = Vec::with_capacity(self.items.len() * 2);
        for (row, col, s) in &self.items {
            orders.push(Order::SetBufferAddress {
                row: *row,
                col: *col,
            });
            orders.push(Order::Text(s.clone()));
        }
        DataStream {
            command: Command::Write,
            wcc: Wcc::NONE,
            width: cols,
            orders,
        }
    }
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows,
            cols,
            orders: Vec::new(),
            inputs: Vec::new(),
            cursor: None,
        }
    }

    /// Geschuetztes Textfeld ab (row, col). Der Text wird auf die Zeile gekappt.
    pub fn text(&mut self, row: u16, col: u16, s: &str, fa: FieldAttribute, color: Option<Color>) {
        let max = self.cols.saturating_sub(col + 1) as usize;
        let s = clip(s, max);
        self.orders.push(Order::SetBufferAddress { row, col });
        self.orders.push(Order::StartField(fa));
        if let Some(c) = color {
            self.orders
                .push(Order::SetAttribute(Attribute::Foreground(c)));
            self.orders.push(Order::Text(s));
            self.orders.push(Order::SetAttribute(Attribute::Reset));
        } else {
            self.orders.push(Order::Text(s));
        }
    }

    /// Ungeschuetztes Eingabefeld der Laenge `len` bei (row, col). Das
    /// Attributbyte steht bei `col`, die Daten ab `col + 1`.
    pub fn input(&mut self, id: FieldId, row: u16, col: u16, len: u16, initial: &str) {
        self.input_with(
            id,
            row,
            col,
            len,
            initial,
            FieldAttribute::unprotected_normal(),
        );
    }

    /// Wie `input`, aber mit eigenem Feldattribut (z. B. numerisch, versteckt).
    pub fn input_with(
        &mut self,
        id: FieldId,
        row: u16,
        col: u16,
        len: u16,
        initial: &str,
        fa: FieldAttribute,
    ) {
        let data = pad(initial, len as usize);
        self.orders.push(Order::SetBufferAddress { row, col });
        self.orders.push(Order::StartField(fa));
        // Eingabefelder: Tuerkis + unterstrichen (siehe docs/STYLE_GUIDE.md)
        self.orders
            .push(Order::SetAttribute(Attribute::Foreground(Color::Turquoise)));
        self.orders
            .push(Order::SetAttribute(Attribute::Highlighting(
                Highlight::Underscore,
            )));
        self.orders.push(Order::Text(data));
        self.orders.push(Order::SetAttribute(Attribute::Reset));
        self.inputs.push(InputField {
            id,
            pos: row as usize * self.cols as usize + col as usize,
            row,
            col,
            len,
        });
    }

    /// Legt Formularfelder untereinander ab `top` an (Beschriftung links,
    /// Eingabe ab [`INPUT_COL`]) und liefert die naechste freie Zeile.
    pub fn form(&mut self, top: u16, fields: &[Field]) -> u16 {
        let mut row = top;
        for f in fields {
            self.text(row, LABEL_COL, f.label, FieldAttribute::normal(), None);
            let fa = match f.kind {
                FieldKind::Number => FieldAttribute::unprotected_normal().numeric(),
                FieldKind::Secret => FieldAttribute::unprotected_hidden(),
                _ => FieldAttribute::unprotected_normal(),
            };
            self.input_with(FieldId::Named(f.key), row, INPUT_COL, f.len, &f.value, fa);
            row += 1;
        }
        row
    }

    /// Schreibt Beschriftung/Wert-Paare untereinander ab `top`. Die Werte
    /// stehen alle in einer Spalte, direkt nach der laengsten Beschriftung.
    /// Ueberlange Werte werden am Zeilenende gekuerzt. Liefert die naechste
    /// freie Zeile.
    pub fn lines(&mut self, top: u16, pairs: &[(&str, &str)]) -> u16 {
        let label_w = pairs
            .iter()
            .map(|(l, _)| l.chars().count())
            .max()
            .unwrap_or(0);
        let value_col = LABEL_COL + label_w as u16 + 2;
        let mut row = top;
        for (label, value) in pairs {
            self.text(
                row,
                LABEL_COL,
                &pad(label, label_w),
                FieldAttribute::normal(),
                None,
            );
            self.text(row, value_col, value, FieldAttribute::normal(), None);
            row += 1;
        }
        row
    }

    /// Setzt den Cursor auf den Datenbeginn eines vorhandenen Eingabefeldes.
    pub fn focus(&mut self, id: FieldId) {
        if let Some(f) = self.inputs.iter().find(|f| f.id == id) {
            self.cursor = Some((f.row, f.col + 1));
        }
    }

    /// Eingabefeld zu einer Id, falls vorhanden.
    pub fn input_of(&self, id: FieldId) -> Option<&InputField> {
        self.inputs.iter().find(|f| f.id == id)
    }

    pub fn to_stream(&self) -> DataStream {
        let mut orders = self.orders.clone();
        if let Some((row, col)) = self.cursor {
            orders.push(Order::SetBufferAddress { row, col });
            orders.push(Order::InsertCursor);
        }
        DataStream {
            command: Command::EraseWriteAlternate,
            wcc: Wcc::reset_restore_mdt(),
            width: self.cols,
            orders,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: FieldKind, len: u16) -> Field {
        Field {
            key: "k",
            label: "Feld",
            len,
            kind,
            value: String::new(),
        }
    }

    #[test]
    fn text_accepts_anything_trimmed() {
        assert_eq!(
            field(FieldKind::Text, 10).check("  Film  ").unwrap(),
            "Film"
        );
    }

    #[test]
    fn number_accepts_integers_and_empty_only() {
        let f = field(FieldKind::Number, 4);
        assert_eq!(f.check("42").unwrap(), "42");
        assert_eq!(f.check("-7").unwrap(), "-7");
        assert_eq!(f.check("").unwrap(), "");
        assert!(f.check("4a").is_err());
        assert!(f.check("1.5").is_err());
    }

    #[test]
    fn choice_is_case_insensitive_and_returns_canonical_value() {
        let f = field(FieldKind::Choice(&["J", "N"]), 1);
        assert_eq!(f.check("n").unwrap(), "N");
        assert!(f.check("X").unwrap_err().contains("J, N"));
        assert!(f.check("").is_err());
    }

    #[test]
    fn lines_align_values_and_clip_at_line_end() {
        let mut s = Screen::new(24, 30);
        let next = s.lines(2, &[("Name", "Film"), ("Laenge", &"x".repeat(50))]);
        assert_eq!(next, 4);
        // Werte beginnen nach der laengsten Beschriftung (6) + 2 Leerzeichen
        let cols: Vec<u16> = s
            .orders
            .iter()
            .filter_map(|o| match o {
                Order::SetBufferAddress { col, .. } => Some(*col),
                _ => None,
            })
            .collect();
        assert_eq!(
            cols,
            vec![LABEL_COL, LABEL_COL + 8, LABEL_COL, LABEL_COL + 8]
        );
        // Der lange Wert wird auf die Zeilenbreite gekuerzt (30 - 10 - 1 = 19 Zeichen)
        let long = s
            .orders
            .iter()
            .filter_map(|o| match o {
                Order::Text(t) if t.starts_with('x') => Some(t.len()),
                _ => None,
            })
            .next();
        assert_eq!(long, Some(19));
    }

    #[test]
    fn form_places_labels_and_inputs_one_row_each() {
        let mut s = Screen::new(24, 80);
        let fields = [
            Field {
                key: "a",
                label: "Titel",
                len: 10,
                kind: FieldKind::Text,
                value: "Film".into(),
            },
            Field {
                key: "b",
                label: "Nummer",
                len: 3,
                kind: FieldKind::Number,
                value: "7".into(),
            },
        ];
        assert_eq!(s.form(3, &fields), 5);
        assert_eq!(s.inputs.len(), 2);
        assert_eq!(
            (
                s.inputs[0].id,
                s.inputs[0].row,
                s.inputs[0].col,
                s.inputs[0].len
            ),
            (FieldId::Named("a"), 3, INPUT_COL, 10)
        );
        assert_eq!(
            (s.inputs[1].id, s.inputs[1].row, s.inputs[1].col),
            (FieldId::Named("b"), 4, INPUT_COL)
        );
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    #[test]
    fn patch_uses_write_without_reset_or_cursor() {
        let mut p = Patch::new();
        p.text(0, 60, "12:34:56", 8);
        let s = p.to_stream(80);
        let bytes = s.to_bytes();
        assert_eq!(bytes[0], 0xF1, "Write-Befehl erwartet");
        assert_eq!(bytes[1], 0x80, "WCC ohne Reset, ohne MDT-Reset");
        assert!(
            s.orders
                .iter()
                .all(|o| matches!(o, Order::SetBufferAddress { .. } | Order::Text(_))),
            "nur SBA und Text erlaubt (kein SF, kein IC)"
        );
    }

    #[test]
    fn patch_pads_to_length_so_old_chars_are_overwritten() {
        let mut p = Patch::new();
        p.text(0, 0, "ab", 4);
        p.text(1, 0, "abcdef", 3);
        let texts: Vec<String> = p
            .to_stream(80)
            .orders
            .iter()
            .filter_map(|o| match o {
                Order::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["ab  ".to_string(), "abc".to_string()]);
    }
}

#[cfg(test)]
mod marker_tests {
    use super::data_marker;

    #[test]
    fn marker_fuellt_breite() {
        for w in [40usize, 80, 132] {
            let m = data_marker("Top of Data", w);
            assert_eq!(m.chars().count(), w);
            assert!(m.starts_with('*') && m.ends_with('*'));
            assert!(m.contains(" Top of Data "));
        }
    }
}
