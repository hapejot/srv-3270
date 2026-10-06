use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------
// 3270-Datenstrom: Command-/Order-Codes
// ---------------------------------------------------------------------
const CMD_ERASE_WRITE: u8 = 0xF5;
const ORDER_SF: u8 = 0x1D; // Start Field
const ORDER_SBA: u8 = 0x11; // Set Buffer Address
const ORDER_IC: u8 = 0x13; // Insert Cursor

const WCC_UNLOCK_RESET_MDT: u8 = 0xC3;

const ATTR_PROTECTED_NORMAL: u8 = 0xC0;
const ATTR_UNPROTECTED_NORMAL: u8 = 0x40;
const ATTR_UNPROTECTED_HIDDEN: u8 = 0x4D;

const AID_ENTER: u8 = 0x7D;
const AID_CLEAR: u8 = 0x6D;
const AID_PF3: u8 = 0xF3;

// 12-Bit-Pufferadresse -> 2 Bytes (reicht bis 4095 Zellen, z.B. Model 2: 24x80=1920)
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

fn rowcol(row: u16, col: u16) -> u16 {
    row * 80 + col // Model 2: 80 Spalten pro Zeile
}

// ---------------------------------------------------------------------
// Sehr reduzierte EBCDIC-Konvertierung (CP037-Grundzeichen: A-Z, a-z, 0-9,
// Leerzeichen und ein paar Satzzeichen). Reicht fuer diese Demo; fuer echte
// Projekte lieber eine vollstaendige Codepage-Crate verwenden.
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
        _ => 0x40, // Fallback: Leerzeichen fuer nicht abgedeckte Zeichen
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
// Login-Screen aufbauen. `username_prefill` haelt die zuletzt eingegebene
// (ungueltige) Eingabe, `error` zeigt ggf. eine Fehlermeldung am unteren
// Bildschirmrand -- so wie es go3270's HandleScreen() mit einem "errorField"
// macht. Da wir bei jedem Aufruf Erase/Write senden, wird der Bildschirm
// ohnehin komplett neu aufgebaut; ein manuelles Loeschen alter Fehlertexte
// ist daher nicht noetig.
// ---------------------------------------------------------------------
fn build_login_screen(username_prefill: &str, error: Option<&str>) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(CMD_ERASE_WRITE);
    out.push(WCC_UNLOCK_RESET_MDT);

    // Label "Benutzer:" bei Zeile 2, Spalte 5
    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(2, 5)));
    out.push(ORDER_SF);
    out.push(ATTR_PROTECTED_NORMAL);
    out.extend(ascii_to_ebcdic("Benutzer:"));

    // Eingabefeld fuer Benutzername (Spalte 15, sichtbar), ggf. vorbelegt
    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(2, 15)));
    out.push(ORDER_SF);
    out.push(ATTR_UNPROTECTED_NORMAL);
    out.extend(ascii_to_ebcdic(username_prefill));

    // Label "Passwort:" bei Zeile 4, Spalte 5
    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(4, 5)));
    out.push(ORDER_SF);
    out.push(ATTR_PROTECTED_NORMAL);
    out.extend(ascii_to_ebcdic("Passwort:"));

    // Eingabefeld fuer Passwort (Spalte 15, hidden/dark) -- wird beim
    // Neuaufbau immer leer angezeigt, auch nach einem fehlgeschlagenen
    // Versuch (Passwoerter nie vorbelegen).
    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(4, 15)));
    out.push(ORDER_SF);
    out.push(ATTR_UNPROTECTED_HIDDEN);

    // Fehlermeldung/Hinweis am unteren Rand, Zeile 22
    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(22, 5)));
    out.push(ORDER_SF);
    out.push(ATTR_PROTECTED_NORMAL);
    let hint = error.unwrap_or("Enter=Anmelden  Clear/PF3=Abbrechen");
    out.extend(ascii_to_ebcdic(hint));

    // Cursor ins Eingabefeld setzen: bei Fehler zurueck zum Benutzername,
    // sonst ebenfalls dorthin (einfachste Variante fuer die Demo)
    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(2, 15)));
    out.push(ORDER_IC);

    out
}

fn build_result_screen(username: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(CMD_ERASE_WRITE);
    out.push(WCC_UNLOCK_RESET_MDT);

    out.push(ORDER_SBA);
    out.extend(encode_address(rowcol(2, 5)));
    out.push(ORDER_SF);
    out.push(ATTR_PROTECTED_NORMAL);
    let text = format!("Hallo {}, Login empfangen.", username);
    out.extend(ascii_to_ebcdic(&text));

    out
}

// ---------------------------------------------------------------------
// Telnet-Negotiation (bewusst naiv, wie auch go3270 es macht: wir gehen
// einfach davon aus, dass der Client mitspielt und pruefen keine Antworten)
// ---------------------------------------------------------------------
const IAC: u8 = 0xFF;
const DO: u8 = 0xFD;
const WILL: u8 = 0xFB;
const EOR: u8 = 0xEF;
const OPT_BINARY: u8 = 0x00;
const OPT_EOR: u8 = 0x19;
const OPT_TERMTYPE: u8 = 0x18;

async fn negotiate_telnet(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.write_all(&[IAC, DO, OPT_BINARY]).await?;
    stream.write_all(&[IAC, WILL, OPT_BINARY]).await?;
    stream.write_all(&[IAC, DO, OPT_EOR]).await?;
    stream.write_all(&[IAC, WILL, OPT_EOR]).await?;
    stream.write_all(&[IAC, DO, OPT_TERMTYPE]).await?;

    // Naiv: wir lesen kurz, was der Client an Optionsantworten/Terminal-Type
    // schickt, und werfen es einfach weg. Fuer ein echtes Projekt sollte
    // hier die Terminal-Type-Subnegotiation sauber ausgewertet werden.
    let mut buf = [0u8; 512];
    let _ = stream.read(&mut buf).await?;
    Ok(())
}

async fn send_3270_record(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    let mut framed = Vec::with_capacity(data.len() + 2);
    for &b in data {
        framed.push(b);
        if b == IAC {
            framed.push(IAC); // 0xFF im Datenteil wird verdoppelt (Telnet-Escaping)
        }
    }
    framed.push(IAC);
    framed.push(EOR);
    stream.write_all(&framed).await
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
                last_was_iac = false;
            } else {
                // andere Telnet-Kommandos ignorieren wir hier naiv
                last_was_iac = false;
            }
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
    Submit { username: String, password: String },
    Exit,
}

fn parse_response(data: &[u8]) -> Option<ClientEvent> {
    if data.len() < 3 {
        return None;
    }
    let aid = data[0];
    // data[1..3] = Cursor-Adresse, fuer die Demo nicht benoetigt

    let mut i = 3;
    let mut fields: Vec<String> = Vec::new();
    while i < data.len() {
        if data[i] == ORDER_SBA {
            i += 3; // SBA + 2 Adressbytes ueberspringen
            let start = i;
            while i < data.len() && data[i] != ORDER_SBA {
                i += 1;
            }
            let text: String = data[start..i]
                .iter()
                .map(|&b| ebcdic_to_ascii(b) as char)
                .collect();
            fields.push(text.trim().to_string());
        } else {
            i += 1;
        }
    }

    match aid {
        AID_ENTER => Some(ClientEvent::Submit {
            username: fields.first().cloned().unwrap_or_default(),
            password: fields.get(1).cloned().unwrap_or_default(),
        }),
        AID_CLEAR | AID_PF3 => Some(ClientEvent::Exit),
        _ => None, // unbekannte/nicht behandelte Taste -> wird als ungueltig gewertet
    }
}

// ---------------------------------------------------------------------
// Verbindung behandeln: Retry-Schleife mit Validierung, angelehnt an
// go3270's HandleScreen() (Screen erneut anzeigen, bis Eingabe gueltig ist
// oder eine Exit-Taste gedrueckt wird).
// ---------------------------------------------------------------------
async fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    println!("Neue Verbindung von {:?}", stream.peer_addr());
    negotiate_telnet(&mut stream).await?;

    let mut username_prefill = String::new();
    let mut error: Option<String> = None;

    loop {
        let screen = build_login_screen(&username_prefill, error.as_deref());
        send_3270_record(&mut stream, &screen).await?;

        let response = read_3270_record(&mut stream).await?;

        match parse_response(&response) {
            Some(ClientEvent::Submit { username, password }) => {
                if username.is_empty() {
                    error = Some("Fehler: Benutzername darf nicht leer sein.".to_string());
                    username_prefill = username;
                    continue;
                }
                if password.is_empty() {
                    error = Some("Fehler: Passwort darf nicht leer sein.".to_string());
                    username_prefill = username;
                    continue;
                }

                println!("Benutzer eingegeben:  {}", username);
                println!("Passwort eingegeben:  {}", password);

                let result_screen = build_result_screen(&username);
                send_3270_record(&mut stream, &result_screen).await?;
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

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:3270").await?;
    println!("TN3270-Demo-Server laeuft auf Port 3270 ...");
    println!("Verbinden z.B. mit: x3270 localhost:3270  (oder c3270 localhost:3270)");

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream).await {
                eprintln!("Fehler bei Verbindung: {e}");
            }
        });
    }
}
