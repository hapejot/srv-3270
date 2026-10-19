//! Bildschirm-Aufbau: geschuetzte Texte und Eingabefelder, uebersetzt in
//! einen 3270-Datenstrom. Eingabefelder sind benannt ([`FieldId`]), damit die
//! Antwort des Terminals einem Feld zugeordnet werden kann.

use crate::datastream::{Command, DataStream, FieldAttribute, Order, Wcc};
use crate::{Attribute, Color};

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
        self.orders.push(Order::Text(data));
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
            (s.inputs[0].id, s.inputs[0].row, s.inputs[0].col, s.inputs[0].len),
            (FieldId::Named("a"), 3, INPUT_COL, 10)
        );
        assert_eq!(
            (s.inputs[1].id, s.inputs[1].row, s.inputs[1].col),
            (FieldId::Named("b"), 4, INPUT_COL)
        );
    }
}
