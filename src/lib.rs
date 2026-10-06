use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
mod datastream;
use datastream::*;

pub use datastream::{
    Attribute, Color, Command, DataStream, FieldAttribute, Highlight, Order, Wcc,
};

// =======================================================================
// Typisiertes Modell des 3270-Datenstroms
//
// Die rohen Byte-/Hex-Werte aus der IBM-3270-Referenz leben jetzt nur noch
// in den jeweiligen `to_byte()`-Methoden dieses Moduls. Wer einen Screen
// beschreiben will, sieht nur noch die Enums -- keine Hex-Konstanten mehr.
// Dank `#[derive(Serialize, Deserialize)]` laesst sich ein `DataStream`
// z.B. als JSON ausgeben (praktisch zum Debuggen) oder sogar aus einer
// Konfigurationsdatei laden.
// =======================================================================
#[derive(Debug)]
pub struct ScreenInfo {
    width: u16,
}

pub trait ScreenTrait: Sync + Send {
    fn data_stream(&self) -> DataStream;
}

#[derive(Clone)]
pub struct Screens {
    screens: Arc<Mutex<HashMap<String, Box<dyn ScreenTrait>>>>,
}

impl Screens {
    pub fn new() -> Self {
        let screens = Arc::new(Mutex::new(HashMap::new()));
        Self { screens }
    }

    pub fn add(&mut self, key: impl Into<String>, screen: Box<dyn ScreenTrait + Sync + Send>) {
        let mut screens = self.screens.try_lock().unwrap();
        screens.insert(key.into(), screen);
    }
}

pub struct Context {
    values: serde_json::Map<String, Value>,
}

impl Context {
    pub fn new() -> Self {
        let values = serde_json::Map::new();
        Self { values }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn get(&mut self, key: impl Into<String>) -> Option<Value> {
        let k: String = key.into();
        self.values.get(&k).cloned()
    }
}

// ---------------------------------------------------------------------
// Low-Level-Bausteine: Order-Codes des 3270-Datenstroms und
// 12-Bit-Pufferadress-Kodierung. Bleiben privat -- der Rest des Programms
// arbeitet nur noch mit den Typen oben.
// ---------------------------------------------------------------------
const ORDER_SF: u8 = 0x1D; // Start Field
const ORDER_SBA: u8 = 0x11; // Set Buffer Address
const ORDER_IC: u8 = 0x13; // Insert Cursor

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

// ---------------------------------------------------------------------
// Sehr reduzierte EBCDIC-Konvertierung (CP037-Grundzeichen). Reicht fuer
// diese Demo; fuer echte Projekte lieber eine vollstaendige Codepage-Crate
// (z.B. `ebcdic` oder `copybook-charset`) einbinden.
// ---------------------------------------------------------------------
fn ascii_to_ebcdic(s: &str) -> Vec<u8> {
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

fn ebcdic_to_ascii(b: u8) -> u8 {
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

// ---------------------------------------------------------------------
const IAC: u8 = 0xFF;
const SE: u8 = 0xF0;
const SB: u8 = 0xFA;
const WILL: u8 = 0xFB;
const WONT: u8 = 0xFC;
const DO: u8 = 0xFD;
const DONT: u8 = 0xFE;
const EOR: u8 = 0xEF;
// const OPT_BINARY: u8 = 0x00;
// const OPT_EOR: u8 = 0x19;
// const OPT_TERMTYPE: u8 = 0x18;

const BINARY_OPTION: u8 = 0x00;
const EOR_OPTION: u8 = 0x19;
const TERMINAL_TYPE: u8 = 0x18;
// const TERMINAL_TYPE_IS: u8 = 0x00;
const TERMINAL_TYPE_SEND: u8 = 0x01;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Telnet error: {0}")]
    Telnet(String),
}

async fn expect_response(stream: &mut TcpStream, verb: u8, opt: u8) -> anyhow::Result<()> {
    loop {
        let mut b = [0u8; 1];
        if stream.read(&mut b).await? == 0 {
            return Err(NetworkError::Telnet("telnet negotiation".into()).into());
        }
        if b[0] != IAC {
            continue; // skip stray bytes
        }
        let mut v = [0u8; 1];
        let mut o = [0u8; 1];
        stream.read_exact(&mut v).await?;
        stream.read_exact(&mut o).await?;
        if v[0] == verb && o[0] == opt {
            return Ok(());
        }
        // Mismatched negotiation — reply with refusal and keep looking.
        match v[0] {
            WILL => {
                let _ = stream.write_all(&[IAC, DONT, o[0]]);
            }
            DO => {
                let _ = stream.write_all(&[IAC, WONT, o[0]]);
            }
            _ => {}
        }
    }
}
fn dimensions_for(term: &str) -> (u16, u16) {
    let base = term.trim_end_matches("-E");
    match base {
        "IBM-3278-2" | "IBM-3279-2" => (24, 80),
        "IBM-3278-3" | "IBM-3279-3" => (32, 80),
        "IBM-3278-4" | "IBM-3279-4" => (43, 80),
        "IBM-3278-5" | "IBM-3279-5" => (27, 132),
        "IBM-DYNAMIC" => (24, 80), // model 2 default; could query for real
        _ => (24, 80),
    }
}
fn parse_terminal_type(buf: &[u8]) -> anyhow::Result<String> {
    // Skip TERMINAL_TYPE (0x18) and IS (0x00) header bytes if present.
    let payload = match buf {
        [0x18, 0x00, rest @ ..] => rest,
        [0x18, rest @ ..] => rest,
        rest => rest,
    };
    let s = std::str::from_utf8(payload)
        .map_err(|e| NetworkError::Telnet(format!("invalid utf-8 in terminal type: {e}")))?;
    Ok(s.trim().to_string())
}
async fn read_terminal_type(stream: &mut TcpStream) -> anyhow::Result<String> {
    // Read until IAC SE, accumulating the TERMINAL_TYPE IS payload.
    let mut state = 0u8; // 0=normal, 1=after IAC
    let mut buf: Vec<u8> = Vec::with_capacity(32);
    let mut started = false;
    loop {
        let mut b = [0u8; 1];
        if stream.read(&mut b).await? == 0 {
            return Err(NetworkError::Telnet("terminal-type read".into()).into());
        }
        match state {
            0 => {
                if b[0] == IAC {
                    state = 1;
                } else if started {
                    buf.push(b[0]);
                }
            }
            1 => {
                match b[0] {
                    SE => return parse_terminal_type(&buf),
                    SB => {
                        started = true;
                        state = 0;
                    }
                    WILL | WONT | DO | DONT => {
                        // Stray option negotiation — consume the option byte and continue.
                        let mut opt = [0u8; 1];
                        stream.read_exact(&mut opt).await?;
                        state = 0;
                    }
                    IAC => {
                        // Escaped 0xFF inside SB payload.
                        if started {
                            buf.push(IAC);
                        }
                        state = 0;
                    }
                    _ => {
                        state = 0;
                    }
                }
            }
            _ => unreachable!(),
        }
    }
}

async fn negotiate_telnet(stream: &mut TcpStream) -> anyhow::Result<ScreenInfo> {
    // Send DO TERMINAL_TYPE and ask the client to send it.
    stream.write_all(&[IAC, DO, TERMINAL_TYPE]).await?;
    expect_response(stream, WILL, TERMINAL_TYPE).await?;

    stream
        .write_all(&[IAC, SB, TERMINAL_TYPE, TERMINAL_TYPE_SEND, IAC, SE])
        .await?;
    let term = read_terminal_type(stream).await?;

    // EOR option (both directions).
    stream.write_all(&[IAC, DO, EOR_OPTION]).await?;
    expect_response(stream, WILL, EOR_OPTION).await?;
    stream.write_all(&[IAC, WILL, EOR_OPTION]).await?;
    expect_response(stream, DO, EOR_OPTION).await?;

    // Binary option (both directions).
    stream.write_all(&[IAC, DO, BINARY_OPTION]).await?;
    expect_response(stream, WILL, BINARY_OPTION).await?;
    stream.write_all(&[IAC, WILL, BINARY_OPTION]).await?;
    expect_response(stream, DO, BINARY_OPTION).await?;

    let (rows, cols) = dimensions_for(&term);
    println!("rows: {rows} / cols: {cols} / term: {term}");
    Ok(ScreenInfo { width: cols })
}

async fn send_3270_record(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let mut framed = Vec::with_capacity(data.len() + 2);
    for &b in data {
        if b == IAC || b == EOR {
            framed.push(IAC);
        }
        framed.push(b);
    }
    framed.push(IAC);
    framed.push(EOR);
    // dump_bytes(&framed);
    stream.write_all(&framed).await
}

fn dump_bytes(framed: &[u8]) {
    let s = framed
        .iter()
        .map(|x| format!("0x{x:02X}"))
        .collect::<Vec<_>>()
        .join(",");
    println!("DUMP: {s}");
}

async fn read_3270_record(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    let mut last_was_iac = false;
    loop {
        stream.read_exact(&mut byte).await?;
        let b = byte[0];
        if last_was_iac {
            if b == EOR {
                break;
            } else if b == IAC {
                out.push(IAC);
            }
            last_was_iac = false;
        } else if b == IAC {
            last_was_iac = true;
        } else {
            out.push(b);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// Antwort des Clients auswerten
// ---------------------------------------------------------------------
enum ClientEvent {
    Submit { fields: Vec<(u16, u16, String)> },
    Exit,
}

fn parse_response(width: u16, data: &[u8]) -> Option<ClientEvent> {
    dump_bytes(data);
    if data.len() < 3 {
        return None;
    }
    let aid = Aid::from_byte(data[0]);
    let (r, c) = buffer_address_to_coordinate(width, &data[1..3]);
    println!("AID: {:02X} {r}x{c}", data[0]);

    let mut i = 3;
    let mut fields: Vec<(u16, u16, String)> = Vec::new();
    while i < data.len() {
        if data[i] == ORDER_SBA {
            let (r, c) = buffer_address_to_coordinate(width, &data[(i + 1)..(i + 3)]);
            println!("SBA {r}x{c}");
            i += 3;
            let start = i;
            while i < data.len() && data[i] != ORDER_SBA {
                i += 1;
            }
            let text: String = data[start..i]
                .iter()
                .map(|&b| ebcdic_to_ascii(b) as char)
                .collect();
            fields.push((r, c, text.trim().to_string()));
        } else {
            i += 1;
        }
    }

    match aid {
        Aid::Enter => Some(ClientEvent::Submit { fields }),
        Aid::Clear | Aid::Pf3 => Some(ClientEvent::Exit),
        _ => None,
    }
}

fn buffer_address_to_coordinate(width: u16, data: &[u8]) -> (u16, u16) {
    let (r, c) = {
        let pos = u16::from_be_bytes([data[0], data[1]]);
        (pos / width, pos % width)
    };
    (r, c)
}

enum StructuredField {
    ImplicitPartition {
        default_width: u16,
        default_height: u16,
        alternate_width: u16,
        alternate_height: u16,
    },
}
impl StructuredField {
    fn to_bytes(&self) -> Vec<u8> {
        todo!()
    }
}

fn structured_field_to_bytes(flds: Vec<StructuredField>) -> anyhow::Result<Vec<u8>> {
    let mut r = vec![];
    for f in flds {
        let b = f.to_bytes();
        r.push((b.len() >> 8) as u8);
        r.push((b.len() & 0xff) as u8);
        r.extend(b);
    }
    Ok(r)
}

// ---------------------------------------------------------------------
// Verbindung behandeln
// ---------------------------------------------------------------------

pub async fn handle_client(
    mut stream: TcpStream,
    screens: &Screens,
    context: &mut Context,
) -> anyhow::Result<()> {
    println!("Neue Verbindung von {:?}", stream.peer_addr());
    let mut term = negotiate_telnet(&mut stream).await?;
    for sf in query_client(&mut stream).await? {
        match sf {
            StructuredField::ImplicitPartition {
                alternate_width, ..
            } => term.width = alternate_width,
        }
    }
    // dump_bytes(&response);

    let mut username_prefill = String::new();
    let mut error: Option<String> = None;

    loop {
        let screen = login_screen(term.width, &username_prefill, error.as_deref());
        send_3270_record(&mut stream, &screen.to_bytes()).await?;

        let response = read_3270_record(&mut stream).await?;

        match parse_response(term.width, &response) {
            Some(ClientEvent::Submit { fields }) => {
                // if username.is_empty() {
                //     error = Some("Fehler: Benutzername darf nicht leer sein.".to_string());
                //     username_prefill = username;
                //     continue;
                // }
                // if password.is_empty() {
                //     error = Some("Fehler: Passwort darf nicht leer sein.".to_string());
                //     username_prefill = username;
                //     continue;
                // }
                for (r, c, t) in fields {
                    println!("{r} {c} '{t}'");
                }

                send_3270_record(&mut stream, &result_screen(term.width, "user").to_bytes())
                    .await?;
                break;
            }
            Some(ClientEvent::Exit) => {
                println!("Verbindung durch Client abgebrochen (Clear/PF3).");
                break;
            }
            None => {
                error = Some("Unerwartete Eingabe, bitte erneut versuchen.".to_string());
                continue;
            }
        }
    }

    Ok(())
}

async fn query_client(stream: &mut TcpStream) -> anyhow::Result<Vec<StructuredField>> {
    send_3270_record(stream, &[0xf3, 0x00, 0x05, 0x01, 0xff, 0x02]).await?;
    println!("sent query");
    let response = read_3270_record(stream).await?;
    if response[0] == 0x88 {
        return Ok(parse_structured_fields(&response[1..]));
    };
    Ok(vec![])
}

fn parse_structured_fields(buf: &[u8]) -> Vec<StructuredField> {
    let mut r = vec![];
    let mut i = 0;
    while i + 3 < buf.len() {
        let l = ((buf[i] as usize) << 8) + buf[i + 1] as usize;
        let c0 = buf[i + 2];
        if c0 == 0x81 {
            let c1 = buf[i + 3];
            match c1 {
                0xa6 => {
                    let data = &buf[(i + 6)..(i + 6 + 0xb)];
                    // dump_bytes(data);
                    let wd = data[3] as u16 * 0x100 + data[4] as u16;
                    let hd = data[5] as u16 * 0x100 + data[6] as u16;
                    let wa = data[7] as u16 * 0x100 + data[8] as u16;
                    let ha = data[9] as u16 * 0x100 + data[10] as u16;
                    println!("wd:{wd} hd:{hd}    wa:{wa}  ha:{ha}");
                    r.push(StructuredField::ImplicitPartition {
                        default_width: wd,
                        default_height: hd,
                        alternate_width: wa,
                        alternate_height: ha,
                    });
                }
                _ => {}
            }
        }
        i += l;
    }
    r
}

// #[tokio::main]
// async fn main() -> std::io::Result<()> {
//     // Kurze Demonstration: der Login-Screen als JSON -- zeigt, dass
//     // DataStream/Order jetzt eine vollstaendig beschreibbare (und damit
//     // auch loggbare/testbare) Struktur sind statt roher Bytes.
//     // let demo = login_screen(80, "", None);
//     // println!(
//     //     "Beispiel-Datenstrom als JSON:\n{}\n",
//     //     serde_json::to_string_pretty(&demo).unwrap()
//     // );

//     let listener = TcpListener::bind("0.0.0.0:3270").await?;
//     println!("TN3270-Demo-Server laeuft auf Port 3270 ...");

//     loop {
//         let (stream, _) = listener.accept().await?;
//         tokio::spawn(async move {
//             if let Err(e) = handle_client(stream).await {
//                 eprintln!("Fehler bei Verbindung: {e}");
//             }
//         });
//     }
// }
