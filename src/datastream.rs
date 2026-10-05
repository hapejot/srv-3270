//! Typisiertes Modell des 3270-Datenstroms.
//!
//! Die rohen Byte-/Hex-Werte aus der IBM-3270-Referenz (GA23-0059-07) leben
//! nur noch in den `to_byte()`/`to_bytes()`-Methoden dieses Moduls. Wer einen
//! Screen beschreiben will, sieht nur noch die Enums -- keine Hex-Konstanten
//! mehr. Dank `#[derive(Serialize, Deserialize)]` laesst sich ein
//! `DataStream` z.B. als JSON ausgeben oder aus einer Konfigurationsdatei
//! laden.
//!
//! Dieses Modul kennt kein TCP/Telnet -- es erzeugt und liest ausschliesslich
//! Byte-Vektoren. Senden/Empfangen ist Sache des aufrufenden Codes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Command {
    /// Bildschirm loeschen und neu beschreiben, Default-Groesse (0xF5)
    EraseWrite,
    /// Wie EraseWrite, aber auf die zuvor per Read-Partition-Query
    /// ermittelte Alternate-Groesse (0x7E)
    EraseWriteAlternate,
}

impl Command {
    fn to_byte(self) -> u8 {
        match self {
            Command::EraseWrite => 0xF5,
            Command::EraseWriteAlternate => 0x7E,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Wcc {
    /// Tastatur entriegeln + MDT aller Felder zuruecksetzen (0xC3) --
    /// der ueblicherweise verwendete WCC-Wert nach einem Erase/Write.
    UnlockKeyboardResetMdt,
}

impl Wcc {
    fn to_byte(self) -> u8 {
        match self {
            Wcc::UnlockKeyboardResetMdt => 0xC3,
        }
    }
}

/// Basis-Feldattribut, bit-genau nach GA23-0059-07 Kapitel 4 (S. 4-13/4-14):
/// Byte = 0x40 (Basiswert) | 0x80 falls protected | Display-Bits | 0x01 falls MDT.
/// (Neu geschriebene Felder starten immer mit MDT=0, daher hier nicht als
/// Option exponiert.)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Protection {
    Protected,
    Unprotected,
}

/// Darstellung/Detektierbarkeit des Feldes (Bits 4-5 des Attributbytes).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Display {
    /// Normal sichtbar, nicht Lichtgriffel-detektierbar
    Normal,
    /// Normal sichtbar, Lichtgriffel-detektierbar
    Detectable,
    /// Hervorgehoben (hell) dargestellt, automatisch detektierbar
    Intensified,
    /// Nicht angezeigt -- fuer Passwoerter o.ae.
    NonDisplay,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FieldAttribute {
    pub protection: Protection,
    pub display: Display,
}

impl FieldAttribute {
    pub const fn protected_normal() -> Self {
        FieldAttribute {
            protection: Protection::Protected,
            display: Display::Normal,
        }
    }
    pub const fn protected_detectable() -> Self {
        FieldAttribute {
            protection: Protection::Protected,
            display: Display::Detectable,
        }
    }
    pub const fn protected_intensified() -> Self {
        FieldAttribute {
            protection: Protection::Protected,
            display: Display::Intensified,
        }
    }
    pub const fn unprotected_normal() -> Self {
        FieldAttribute {
            protection: Protection::Unprotected,
            display: Display::Normal,
        }
    }
    pub const fn unprotected_hidden() -> Self {
        FieldAttribute {
            protection: Protection::Unprotected,
            display: Display::NonDisplay,
        }
    }

    fn to_byte(self) -> u8 {
        let mut b: u8 = 0x00;
        if matches!(self.protection, Protection::Protected) {
            b |= 0x20;
        }
        b |= match self.display {
            Display::Normal => 0x00,
            Display::Detectable => 0x04,
            Display::Intensified => 0x08,
            Display::NonDisplay => 0x0C,
        };
        b
    }
}

/// Highlighting-Werte fuer SetAttribute (Typ X'41'), S. 4-18.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Highlight {
    Default,
    Normal,
    Blink,
    ReverseVideo,
    Underscore,
}

impl Highlight {
    fn to_byte(self) -> u8 {
        match self {
            Highlight::Default => 0x00,
            Highlight::Normal => 0xF0,
            Highlight::Blink => 0xF1,
            Highlight::ReverseVideo => 0xF2,
            Highlight::Underscore => 0xF4,
        }
    }
}

/// Farbwerte fuer SetAttribute (Vordergrund X'42' / Hintergrund X'45'),
/// Tabelle 4-7, S. 4-19.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Color {
    Default,
    Neutral,
    Blue,
    Red,
    Pink,
    Green,
    Turquoise,
    Yellow,
    NeutralWhite,
    Black,
    DeepBlue,
    Orange,
    Purple,
    PaleGreen,
    PaleTurquoise,
    Grey,
    White,
}

impl Color {
    fn to_byte(self) -> u8 {
        match self {
            Color::Default => 0x00,
            Color::Neutral => 0xF0,
            Color::Blue => 0xF1,
            Color::Red => 0xF2,
            Color::Pink => 0xF3,
            Color::Green => 0xF4,
            Color::Turquoise => 0xF5,
            Color::Yellow => 0xF6,
            Color::NeutralWhite => 0xF7,
            Color::Black => 0xF8,
            Color::DeepBlue => 0xF9,
            Color::Orange => 0xFA,
            Color::Purple => 0xFB,
            Color::PaleGreen => 0xFC,
            Color::PaleTurquoise => 0xFD,
            Color::Grey => 0xFE,
            Color::White => 0xFF,
        }
    }
}

/// Ein Typ-Wert-Paar fuer die SA-Order (S. 4-6: X'28' + Typ + Wert, genau
/// 3 Bytes -- mehrere gleichzeitig aktive Attribute werden durch mehrere
/// aufeinanderfolgende SA-Orders erreicht, nicht durch ein kombiniertes SF).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Attribute {
    /// Setzt alle Zeichenattribute (Farbe, Highlighting, ...) auf Default zurueck
    Reset,
    Highlighting(Highlight),
    Foreground(Color),
    Background(Color),
}

impl Attribute {
    fn to_type_value(self) -> (u8, u8) {
        match self {
            Attribute::Reset => (0x00, 0x00),
            Attribute::Highlighting(h) => (0x41, h.to_byte()),
            Attribute::Foreground(c) => (0x42, c.to_byte()),
            Attribute::Background(c) => (0x45, c.to_byte()),
        }
    }
}

/// Eine einzelne Anweisung im Datenstrom.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Order {
    /// Setzt die Schreibposition auf dem Bildschirm (1-basiert waere auch
    /// moeglich, hier bewusst 0-basiert wie im Buffer selbst)
    SetBufferAddress { row: u16, col: u16 },
    /// Startet ein neues Feld mit dem angegebenen Attribut
    StartField(FieldAttribute),
    /// Setzt den Cursor auf die zuletzt gesetzte Bufferadresse
    InsertCursor,
    /// Reiner Text, wird beim Encodieren automatisch EBCDIC-kodiert
    Text(String),
    /// Setzt ein Zeichenattribut (Highlighting/Farbe), gilt fuer alle
    /// nachfolgenden Zeichen bis zum naechsten SetAttribute/Write/Clear
    SetAttribute(Attribute),
    /// Fuellt den Buffer ab der aktuellen Position mit `fill` bis
    /// einschliesslich (row, col)
    RepeatToAddress { row: u16, col: u16, fill: char },
    /// Schreibt `count` rohe Null-Bytes (X'00') in den Buffer
    Nulls(usize),
}

/// Ein kompletter, sendefertiger Bildschirm-Aufbau.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStream {
    pub command: Command,
    pub wcc: Wcc,
    /// Bildschirmbreite (Spalten), die fuer die SBA-Adressrechnung dieses
    /// Screens gilt -- Default 80, nach erfolgreicher Implicit-Partition-
    /// Query ggf. die ermittelte Alternate-Breite.
    pub width: u16,
    pub orders: Vec<Order>,
}

impl DataStream {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.command.to_byte());
        out.push(self.wcc.to_byte());
        for order in &self.orders {
            match order {
                Order::SetBufferAddress { row, col } => {
                    out.push(ORDER_SBA);
                    out.extend(encode_address(rowcol(*row, *col, self.width)));
                }
                Order::StartField(attr) => {
                    out.push(ORDER_SF);
                    out.push(attr.to_byte());
                }
                Order::InsertCursor => out.push(ORDER_IC),
                Order::Text(text) => out.extend(ascii_to_ebcdic(text)),
                Order::SetAttribute(attr) => {
                    let (attr_type, value) = attr.to_type_value();
                    out.push(ORDER_SA);
                    out.push(attr_type);
                    out.push(value);
                }
                Order::RepeatToAddress { row, col, fill } => {
                    out.push(ORDER_RA);
                    out.extend(encode_address(rowcol(*row, *col, self.width)));
                    out.push(ascii_byte_to_ebcdic(*fill as u8));
                }
                Order::Nulls(count) => out.extend(std::iter::repeat(0x00u8).take(*count)),
            }
        }
        out
    }
}

/// Attention-Identifier, den der Client beim Enter/PF-Tastendruck zurueckschickt.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aid {
    Enter,
    Clear,
    Pf1,
    Pf2,
    Pf3,
    Pf4,
    Pf5,
    Pf6,
    Pf7,
    Pf8,
    Pf9,
    Pf10,
    Pf11,
    Pf12,
    Pf13,
    Pf14,
    Pf15,
    Pf16,
    Pf17,
    Pf18,
    Pf19,
    Pf20,
    Pf21,
    Pf22,
    Pf23,
    Pf24,
    /// keine normale Tasteneingabe
    StructuredField,
    Unknown,
}

impl Aid {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x7D => Aid::Enter,
            0x6D => Aid::Clear,
            0xF1 => Aid::Pf1,
            0xF2 => Aid::Pf2,
            0xF3 => Aid::Pf3,
            0xF4 => Aid::Pf4,
            0xF5 => Aid::Pf5,
            0xF6 => Aid::Pf6,
            0xF7 => Aid::Pf7,
            0xF8 => Aid::Pf8,
            0xF9 => Aid::Pf9,
            0xFA => Aid::Pf10,
            0xFB => Aid::Pf11,
            0xFC => Aid::Pf12,
            0xC1 => Aid::Pf13,
            0xC2 => Aid::Pf14,
            0xC3 => Aid::Pf15,
            0xC4 => Aid::Pf16,
            0xC5 => Aid::Pf17,
            0xC6 => Aid::Pf18,
            0xC7 => Aid::Pf19,
            0x88 => Aid::StructuredField,
            _ => Aid::Unknown,
        }
    }
}

// ---------------------------------------------------------------------
// Low-Level-Bausteine: Order-Codes des 3270-Datenstroms und
// 12-Bit-Pufferadress-Kodierung. ORDER_SBA ist pub, da aufrufender Code
// (z.B. das Parsen der Inbound-Antwort) selbst nach SBA-Bytes scannen muss;
// der Rest bleibt intern.
// ---------------------------------------------------------------------
pub const ORDER_SBA: u8 = 0x11; // Set Buffer Address
const ORDER_SF: u8 = 0x1D; // Start Field
const ORDER_IC: u8 = 0x13; // Insert Cursor
const ORDER_SA: u8 = 0x28; // Set Attribute
const ORDER_RA: u8 = 0x3C; // Repeat to Address

// --- Read Partition Query / Query Reply (Implicit Partition) ---
// Werte verifiziert gegen GA23-0059-07, Kapitel 5 (S. 5-51) und Kapitel 6 (S. 6-71/6-72).
const CMD_WSF: u8 = 0xF3; // Write Structured Field
const SFID_READ_PARTITION: u8 = 0x01;
const RP_TYPE_QUERY_LIST: u8 = 0x03;
const RP_REQTYP_QCODE_LIST: u8 = 0x00; // Bits 0-1 von Byte 5
const QCODE_IMPLICIT_PARTITION: u8 = 0xA6;
const SFID_QUERY_REPLY: u8 = 0x81;
const SDPID_IMPLICIT_PARTITION_SIZES: u8 = 0x01;

const ADDR_TABLE: [u8; 64] = [
    0x40, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F,
    0x50, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F,
    0x60, 0x61, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F,
    0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0x7A, 0x7B, 0x7C, 0x7D, 0x7E, 0x7F,
];

fn encode_address(addr: u16) -> [u8; 2] {
    let hi = ((addr >> 6) & 0x3F) as usize;
    let lo = (addr & 0x3F) as usize;
    [ADDR_TABLE[hi], ADDR_TABLE[lo]]
}

fn rowcol(row: u16, col: u16, width: u16) -> u16 {
    row * width + col
}

// ---------------------------------------------------------------------
// Read Partition Query (outbound) -- fragt die Implicit-Partition-Groesse
// (Default/Alternate) des Terminals ab. Muss VOR dem ersten EW/EWA gesendet
// werden, da EWA selbst keinerlei Groesseninformation traegt. Reine
// Byte-Erzeugung/-Auswertung -- das Senden/Lesen macht der aufrufende Code.
// ---------------------------------------------------------------------
pub fn build_read_partition_query_implicit_partition() -> Vec<u8> {
    // Struktur laut GA23-0059-07, S. 5-51:
    // 0-1 Laenge | 2 SFID=01 | 3 PID=FF | 4 TYPE=03 (Query List)
    // 5 REQTYP (Bits 0-1)=00 (QCODE List) | 6..n QCODE-Liste
    let sf: [u8; 7] = [
        0x00,
        0x07, // Laenge dieses SF (7 Bytes, inkl. der 2 Laengenbytes selbst)
        SFID_READ_PARTITION,
        0xFF, // PID: Query-Operation
        RP_TYPE_QUERY_LIST,
        RP_REQTYP_QCODE_LIST,
        QCODE_IMPLICIT_PARTITION,
    ];
    let mut out = vec![CMD_WSF];
    out.extend(sf);
    out
}

/// Wertet eine Query-Reply-Antwort aus und liefert (Breite, Hoehe) der
/// Alternate-Groesse, falls eine Query Reply (Implicit Partition) enthalten
/// war. Struktur laut GA23-0059-07, S. 6-71/6-72:
/// Byte 2 SFID=81, Byte 3 QCODE=A6, Byte 4-5 Flags, danach Self-Defining
/// Parameter mit SDPID=01: Byte 3-4=WD, 5-6=HD, 7-8=WA, 9-10=HA
/// (Offsets relativ zum Start des Self-Defining Parameters).
pub fn parse_implicit_partition_reply(data: &[u8]) -> Option<(u16, u16)> {
    if data.is_empty() || Aid::from_byte(data[0]) != Aid::StructuredField {
        return None; // keine Structured-Field-Antwort (z.B. normale Tasteneingabe)
    }

    let mut i = 1;
    while i + 4 <= data.len() {
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        if len < 6 || i + len > data.len() {
            break; // defekte/unbekannte Laenge -- Abbruch statt Fehlinterpretation
        }
        let sfid = data[i + 2];
        let qcode = data[i + 3];

        if sfid == SFID_QUERY_REPLY && qcode == QCODE_IMPLICIT_PARTITION {
            // Self-Defining Parameter(e) beginnen direkt nach den Flags (Byte 4-5)
            let mut j = i + 6;
            while j + 2 <= i + len {
                let sdp_len = data[j] as usize;
                if sdp_len < 3 || j + sdp_len > i + len {
                    break;
                }
                let sdpid = data[j + 1];
                if sdpid == SDPID_IMPLICIT_PARTITION_SIZES && sdp_len >= 11 {
                    let wa = u16::from_be_bytes([data[j + 7], data[j + 8]]);
                    let ha = u16::from_be_bytes([data[j + 9], data[j + 10]]);
                    return Some((wa, ha));
                }
                j += sdp_len;
            }
        }
        i += len; // zum naechsten Structured Field in der Antwortkette
    }
    None
}

// ---------------------------------------------------------------------
// Sehr reduzierte EBCDIC-Konvertierung (CP037-Grundzeichen). Reicht fuer
// diese Demo; fuer echte Projekte lieber eine vollstaendige Codepage-Crate
// (z.B. `ebcdic` oder `copybook-charset`) einbinden. pub, da aufrufender
// Code (z.B. das Parsen der Inbound-Feldwerte) sie ebenfalls braucht.
// ---------------------------------------------------------------------
pub fn ascii_to_ebcdic(s: &str) -> Vec<u8> {
    s.bytes().map(ascii_byte_to_ebcdic).collect()
}

fn ascii_byte_to_ebcdic(b: u8) -> u8 {
    match b {
        b' ' => 0x40,
        b'A'..=b'I' => 0xC1 + (b - b'A'),
        b'J'..=b'R' => 0xD1 + (b - b'J'),
        b'S'..=b'Z' => 0xE2 + (b - b'S'),
        b'a'..=b'i' => 0x81 + (b - b'a'),
        b'j'..=b'r' => 0x91 + (b - b'j'),
        b's'..=b'z' => 0xA2 + (b - b's'),
        b'0'..=b'9' => 0xF0 + (b - b'0'),
        b':' => 0x7A,
        b'.' => 0x4B,
        b',' => 0x6B,
        b'-' => 0x60,
        b'!' => 0x5A,
        b'?' => 0x6F,
        _ => 0x40,
    }
}

pub fn ebcdic_to_ascii(b: u8) -> u8 {
    match b {
        0x40 => b' ',
        0xC1..=0xC9 => b'A' + (b - 0xC1),
        0xD1..=0xD9 => b'J' + (b - 0xD1),
        0xE2..=0xE9 => b'S' + (b - 0xE2),
        0x81..=0x89 => b'a' + (b - 0x81),
        0x91..=0x99 => b'j' + (b - 0x91),
        0xA2..=0xA9 => b's' + (b - 0xA2),
        0xF0..=0xF9 => b'0' + (b - 0xF0),
        0x7A => b':',
        0x4B => b'.',
        0x6B => b',',
        0x60 => b'-',
        0x5A => b'!',
        0x6F => b'?',
        _ => b' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut probeweise einen Ausschnitt aus dem I-MON-Trace nach (Titelzeile
    /// + Menuepunkt "A - ADDRESS SPACE MONITOR"), um zu zeigen, dass sich
    /// SetAttribute/RepeatToAddress/Nulls im Modell ausdruecken lassen.
    #[test]
    fn imon_ausschnitt_laesst_sich_beschreiben() {
        let screen = DataStream {
            command: Command::EraseWrite,
            wcc: Wcc::UnlockKeyboardResetMdt,
            width: 80,
            orders: vec![
                Order::SetBufferAddress { row: 1, col: 1 },
                Order::StartField(FieldAttribute::protected_normal()),
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
            ],
        };

        let bytes = screen.to_bytes();
        assert!(!bytes.is_empty());
        // Stichprobe: Highlighting-SA (X'28' X'41' X'F4' = Underscore) muss enthalten sein
        assert!(bytes.windows(3).any(|w| w == [ORDER_SA, 0x41, 0xF4]));
        // Stichprobe: Foreground-SA fuer Yellow (X'28' X'42' X'F6')
        assert!(bytes.windows(3).any(|w| w == [ORDER_SA, 0x42, 0xF6]));
    }
}
