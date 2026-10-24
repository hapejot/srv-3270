//! Telnet- und TN3270-Transport: Verhandlung, Record-Framing, Adressen,
//! Eingabe-Parsing (Read Modified) und EBCDIC-Dekodierung.

use std::time::Duration;

use anyhow::Result;

use crate::datastream::{
    ADDR_TABLE, ORDER_SBA, build_read_partition_query_implicit_partition,
    parse_implicit_partition_reply,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::io::{AsyncRead, AsyncWrite};

/// Transport einer Sitzung: TCP oder TLS ueber TCP.
pub trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}
pub type BoxedIo = Box<dyn Transport>;
use tokio::time::timeout;

/// Wartezeit auf die Query Reply. Terminals ohne Query-Unterstützung antworten nicht.
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

const IAC: u8 = 0xFF;
const DONT: u8 = 0xFE;
const DO: u8 = 0xFD;
const WONT: u8 = 0xFC;
const WILL: u8 = 0xFB;
const SB: u8 = 0xFA;
const SE: u8 = 0xF0;
const EOR: u8 = 0xEF;

const OPT_BINARY: u8 = 0x00;
const OPT_TTYPE: u8 = 0x18;
const OPT_EOR: u8 = 0x19;
const TTYPE_IS: u8 = 0x00;
const TTYPE_SEND: u8 = 0x01;

/// Eine Telnet-Einheit aus dem Byte-Strom.
enum Event {
    Data,
    Eor,
    Opt(u8, u8),
    Sub(Vec<u8>),
    Other,
}

async fn read_event(io: &mut BufReader<BoxedIo>) -> std::io::Result<Event> {
    let b = io.read_u8().await?;
    if b != IAC {
        return Ok(Event::Data);
    }
    let c = io.read_u8().await?;
    Ok(match c {
        IAC => Event::Data,
        EOR => Event::Eor,
        WILL | WONT | DO | DONT => Event::Opt(c, io.read_u8().await?),
        SB => {
            let mut payload = Vec::new();
            loop {
                let x = io.read_u8().await?;
                if x == IAC {
                    let y = io.read_u8().await?;
                    if y == SE {
                        break;
                    }
                    if y == IAC {
                        payload.push(IAC);
                    }
                } else {
                    payload.push(x);
                }
            }
            Event::Sub(payload)
        }
        _ => Event::Other,
    })
}

/// Zustand des Telnet-Parsers. Liegt im Terminal, damit `read_record` jederzeit
/// abgebrochen werden kann (z. B. in `select!`), ohne Bytes zu verlieren.
#[derive(Default)]
enum PState {
    #[default]
    Data,
    Iac,
    Verb(u8),
    Sb(Vec<u8>),
    SbIac(Vec<u8>),
}

#[derive(Default)]
struct Parser {
    state: PState,
    rec: Vec<u8>,
}

impl Parser {
    /// Verarbeitet ein Byte. Liefert ein Ereignis, sobald eines vollstaendig ist.
    fn feed(&mut self, b: u8) -> Option<Event> {
        match std::mem::take(&mut self.state) {
            PState::Data => {
                if b == IAC {
                    self.state = PState::Iac;
                } else {
                    self.rec.push(b);
                }
                None
            }
            PState::Iac => match b {
                IAC => {
                    self.rec.push(IAC);
                    None
                }
                EOR => Some(Event::Eor),
                WILL | WONT | DO | DONT => {
                    self.state = PState::Verb(b);
                    None
                }
                SB => {
                    self.state = PState::Sb(Vec::new());
                    None
                }
                _ => None,
            },
            PState::Verb(verb) => Some(Event::Opt(verb, b)),
            PState::Sb(mut payload) => {
                if b == IAC {
                    self.state = PState::SbIac(payload);
                } else {
                    payload.push(b);
                    self.state = PState::Sb(payload);
                }
                None
            }
            PState::SbIac(mut payload) => match b {
                SE => None,
                IAC => {
                    payload.push(IAC);
                    self.state = PState::Sb(payload);
                    None
                }
                _ => None,
            },
        }
    }
}

pub struct Terminal {
    io: BufReader<BoxedIo>,
    parser: Parser,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Default)]
struct NegState {
    term: Option<String>,
    eor: bool,
    binary: bool,
}

impl Terminal {
    /// Startet die Telnet-Verhandlung auf einem beliebigen Transport (TCP oder TLS).
    pub async fn negotiate<T: Transport + 'static>(stream: T) -> Result<Self> {
        let mut io = BufReader::new(Box::new(stream) as BoxedIo);
        io.get_mut()
            .write_all(&[
                IAC, DO, OPT_TTYPE, IAC, WILL, OPT_EOR, IAC, DO, OPT_EOR, IAC, WILL, OPT_BINARY,
                IAC, DO, OPT_BINARY,
            ])
            .await?;
        let mut st = NegState::default();
        // Wenn der Client nicht vollstaendig verhandelt, nehmen wir Defaults.
        let _ = timeout(Duration::from_secs(5), negotiate_inner(&mut io, &mut st)).await;
        let term_type = st.term.unwrap_or_else(|| "IBM-3278-2".to_string());
        let (rows, cols) = dimensions_for(&term_type);
        let mut term = Self {
            io,
            parser: Parser::default(),
            rows,
            cols,
        };
        term.detect_size().await?;
        Ok(term)
    }

    /// Ermittelt die Alternate-Groesse per Read Partition Query. Der Terminaltyp
    /// dient nur als Fallback, falls keine gueltige Antwort kommt.
    async fn detect_size(&mut self) -> Result<()> {
        self.write_record(&build_read_partition_query_implicit_partition())
            .await?;
        if let Ok(Ok(Some((cols, rows)))) = timeout(QUERY_TIMEOUT, self.read_query_reply()).await {
            if cols > 0 && rows > 0 {
                self.cols = cols;
                self.rows = rows;
            }
        }
        Ok(())
    }

    /// Verwirft Records, bis eine Implicit-Partition-Antwort eintrifft.
    async fn read_query_reply(&mut self) -> Result<Option<(u16, u16)>> {
        while let Some(rec) = self.read_record().await? {
            if let Some(size) = parse_implicit_partition_reply(&rec) {
                return Ok(Some(size));
            }
        }
        Ok(None)
    }

    /// Liest ein Record bis IAC EOR. `None` = Verbindung geschlossen.
    ///
    /// Cancel-safe: Der Parserzustand liegt im Terminal, jedes Byte wird genau
    /// einmal konsumiert. Die Future darf also jederzeit verworfen werden.
    pub async fn read_record(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            let b = match self.io.read_u8().await {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            match self.parser.feed(b) {
                Some(Event::Eor) => return Ok(Some(std::mem::take(&mut self.parser.rec))),
                // Akzeptierte Optionen nicht beantworten, unbekannte ablehnen.
                Some(Event::Opt(verb, opt)) if !matches!(opt, OPT_BINARY | OPT_TTYPE | OPT_EOR) => {
                    let reply = if verb == WILL { DONT } else { WONT };
                    self.io.get_mut().write_all(&[IAC, reply, opt]).await?;
                }
                _ => {}
            }
        }
    }

    /// Sendet einen 3270-Datenstrom als Record (IAC-Escaping + IAC EOR).
    pub async fn write_record(&mut self, data: &[u8]) -> Result<()> {
        let mut framed = Vec::with_capacity(data.len() + 4);
        for &b in data {
            if b == IAC {
                framed.push(IAC);
            }
            framed.push(b);
        }
        framed.push(IAC);
        framed.push(EOR);
        self.io.get_mut().write_all(&framed).await?;
        self.io.get_mut().flush().await?;
        Ok(())
    }
}

async fn negotiate_inner(io: &mut BufReader<BoxedIo>, st: &mut NegState) -> Result<()> {
    loop {
        if st.term.is_some() && st.eor && st.binary {
            return Ok(());
        }
        match read_event(io).await? {
            Event::Opt(WILL, OPT_TTYPE) => {
                io.get_mut()
                    .write_all(&[IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE])
                    .await?;
            }
            Event::Opt(WILL | DO, OPT_EOR) => st.eor = true,
            Event::Opt(WILL | DO, OPT_BINARY) => st.binary = true,
            Event::Sub(p) if p.first() == Some(&OPT_TTYPE) && p.get(1) == Some(&TTYPE_IS) => {
                st.term = Some(String::from_utf8_lossy(&p[2..]).trim().to_string());
            }
            _ => {}
        }
    }
}

/// Zeilen/Spalten je Terminal-Modell (3278-2 bis -5).
pub fn dimensions_for(term: &str) -> (u16, u16) {
    let base = term.trim_end_matches("-E");
    match base {
        "IBM-3278-3" | "IBM-3279-3" => (32, 80),
        "IBM-3278-4" | "IBM-3279-4" => (43, 80),
        "IBM-3278-5" | "IBM-3279-5" => (27, 132),
        _ => (24, 80),
    }
}

// ---------------------------------------------------------------------
// Buffer-Adressen (12-Bit- und 14-Bit-Kodierung)
// ---------------------------------------------------------------------

fn table_index(b: u8) -> usize {
    ADDR_TABLE.iter().position(|&x| x == b).unwrap_or(0)
}

/// Dekodiert zwei Adressbytes (12-Bit-Codes oder 14-Bit-Binaer) in eine Position.
pub fn decode_address(b1: u8, b2: u8) -> usize {
    if b1 & 0xC0 == 0 {
        (((b1 & 0x3F) as usize) << 8) | b2 as usize
    } else {
        (table_index(b1) << 6) | table_index(b2)
    }
}

// ---------------------------------------------------------------------
// Eingabe (Read Modified)
// ---------------------------------------------------------------------

/// Attention-Identifier: welche Taste die Eingabe ausgeloest hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aid {
    Enter,
    Clear,
    /// PF1..PF24
    Pf(u8),
    /// PA1..PA3
    Pa(u8),
    /// Alles andere (z. B. Structured Field, unbekannte Taste)
    Other,
}

impl Aid {
    /// Liefert die Taste PF`n` fuer `n` in 1..=24, sonst `None`.
    /// `Aid::Pf(n)` bezeichnet dieselbe Taste (PF1..PF24).
    pub fn from_pf(n: u8) -> Option<Aid> {
        (1..=24).contains(&n).then_some(Aid::Pf(n))
    }
}

pub fn aid_from(b: u8) -> Aid {
    match b {
        0x7D => Aid::Enter,
        0x6D => Aid::Clear,
        0xF1..=0xF9 => Aid::Pf(b - 0xF0),
        0x7A => Aid::Pf(10),
        0x7B => Aid::Pf(11),
        0x7C => Aid::Pf(12),
        0xC1..=0xC9 => Aid::Pf(b - 0xC1 + 13),
        0x4A..=0x4C => Aid::Pf(b - 0x4A + 22),
        0x6C => Aid::Pa(1),
        0x6E => Aid::Pa(2),
        0x6B => Aid::Pa(3),
        _ => Aid::Other,
    }
}

#[derive(Debug)]
pub struct Inbound {
    pub aid: Aid,
    /// Cursorposition beim Absenden (Buffer-Adresse), falls mitgeliefert.
    pub cursor: Option<usize>,
    /// (Buffer-Position des Feldbeginns, Text)
    pub fields: Vec<(usize, String)>,
}

pub fn parse_inbound(rec: &[u8]) -> Inbound {
    if rec.is_empty() {
        return Inbound {
            aid: Aid::Other,
            cursor: None,
            fields: vec![],
        };
    }
    let aid = aid_from(rec[0]);
    let cursor = (rec.len() >= 3).then(|| decode_address(rec[1], rec[2]));
    let mut fields = Vec::new();
    let mut i = 3.min(rec.len());
    while i < rec.len() {
        if rec[i] == ORDER_SBA && i + 2 < rec.len() {
            let pos = decode_address(rec[i + 1], rec[i + 2]);
            let start = i + 3;
            let mut j = start;
            while j < rec.len() && rec[j] != ORDER_SBA {
                j += 1;
            }
            let text: String = rec[start..j].iter().map(|&b| ebcdic_to_ascii(b)).collect();
            fields.push((pos, text));
            i = j;
        } else {
            i += 1;
        }
    }
    Inbound { aid, cursor, fields }
}

// ---------------------------------------------------------------------
// EBCDIC (CP037) -> Unicode. Siehe [`crate::ebcdic`].
// ---------------------------------------------------------------------

pub fn ebcdic_to_ascii(b: u8) -> char {
    crate::ebcdic::decode_byte(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fuettert Bytes in den Parser und sammelt abgeschlossene Records.
    fn feed_all(p: &mut Parser, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for &b in bytes {
            if let Some(Event::Eor) = p.feed(b) {
                out.push(std::mem::take(&mut p.rec));
            }
        }
        out
    }

    #[test]
    fn aid_from_pf_accepts_only_1_to_24() {
        assert_eq!(Aid::from_pf(1), Some(Aid::Pf(1)));
        assert_eq!(Aid::from_pf(24), Some(Aid::Pf(24)));
        assert_eq!(Aid::from_pf(0), None);
        assert_eq!(Aid::from_pf(25), None);
    }

    #[test]
    fn parser_splits_records_and_unescapes_iac() {
        let mut p = Parser::default();
        let recs = feed_all(
            &mut Parser::default(),
            &[1, IAC, IAC, 2, IAC, EOR, 3, IAC, EOR],
        );
        assert_eq!(recs, vec![vec![1, IAC, 2], vec![3]]);
        // Auch wenn ein Record in mehreren Teilen ankommt, bleibt der Zustand erhalten.
        assert!(feed_all(&mut p, &[7, 8]).is_empty());
        assert_eq!(feed_all(&mut p, &[IAC, EOR]), vec![vec![7, 8]]);
    }

    #[test]
    fn parser_reports_options_without_data() {
        let mut p = Parser::default();
        assert!(p.feed(IAC).is_none());
        assert!(p.feed(WILL).is_none());
        assert!(matches!(p.feed(0x19), Some(Event::Opt(WILL, 0x19))));
        assert!(p.rec.is_empty());
    }

    #[test]
    fn address_12bit_roundtrip() {
        // 80 (Zeile 1, Spalte 0) bzw. 1919 werden mit der Tabelle kodiert.
        for pos in [0usize, 1, 79, 80, 1919, 4095] {
            let b1 = ADDR_TABLE[(pos >> 6) & 0x3F];
            let b2 = ADDR_TABLE[pos & 0x3F];
            assert_eq!(decode_address(b1, b2), pos);
        }
    }

    #[test]
    fn address_14bit() {
        assert_eq!(decode_address(0x00, 0x50), 0x50);
        assert_eq!(decode_address(0x07, 0xD0), 2000);
    }

    #[test]
    fn ebcdic_letters() {
        assert_eq!(ebcdic_to_ascii(0xC1), 'A');
        assert_eq!(ebcdic_to_ascii(0x81), 'a');
        assert_eq!(ebcdic_to_ascii(0xF0), '0');
        assert_eq!(ebcdic_to_ascii(0x40), ' ');
    }

    #[test]
    fn parse_enter_with_one_field() {
        // AID Enter, Cursor 0, dann SBA auf Position 80 mit "AB"
        let rec = [
            0x7D,
            ADDR_TABLE[0],
            ADDR_TABLE[0],
            ORDER_SBA,
            ADDR_TABLE[1],
            ADDR_TABLE[16],
            0xC1,
            0xC2,
        ];
        let inb = parse_inbound(&rec);
        assert_eq!(inb.aid, Aid::Enter);
        assert_eq!(inb.fields, vec![(16 * 1 + 0 + 64, "AB".to_string())]);
    }

    #[test]
    fn query_reply_yields_alternate_size() {
        // AID 0x88, SF 0x81/QCODE A6, SDP 0x0B/SDPID 01: WD 80x24, WA 80x43 (cols x rows).
        let rec = [
            0x88, 0x00, 0x11, 0x81, 0xA6, 0x00, 0x00, 0x0B, 0x01, 0x00, 0x00, 0x50, 0x00, 0x18,
            0x00, 0x50, 0x00, 0x2B,
        ];
        assert_eq!(parse_implicit_partition_reply(&rec), Some((80, 43)));
    }

    #[test]
    fn non_query_record_is_ignored() {
        assert_eq!(parse_implicit_partition_reply(&[0x7D, 0x40, 0x40]), None);
    }

    /// Simuliert ein Terminal: verhandelt TTYPE/EOR/BINARY und antwortet optional
    /// auf die Read Partition Query mit einer Implicit-Partition-Antwort.
    async fn fake_terminal(addr: std::net::SocketAddr, answer_query: bool) {
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut hello = vec![IAC, WILL, OPT_TTYPE, IAC, SB, OPT_TTYPE, TTYPE_IS];
        hello.extend_from_slice(b"IBM-3278-2");
        hello.extend_from_slice(&[IAC, SE, IAC, WILL, OPT_EOR, IAC, WILL, OPT_BINARY]);
        c.write_all(&hello).await.unwrap();

        // Die Abfrage endet mit IAC EOR; davor liegen nur Telnet-Verhandlungsbytes.
        let mut seen = Vec::new();
        while !seen.ends_with(&[IAC, EOR]) {
            seen.push(c.read_u8().await.unwrap());
        }
        if answer_query {
            let reply = [
                0x88, 0x00, 0x11, 0x81, 0xA6, 0x00, 0x00, 0x0B, 0x01, 0x00, 0x00, 0x50, 0x00, 0x18,
                0x00, 0x50, 0x00, 0x2B, IAC, EOR,
            ];
            c.write_all(&reply).await.unwrap();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    async fn negotiate_with(answer_query: bool) -> Terminal {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(fake_terminal(addr, answer_query));
        let (stream, _) = listener.accept().await.unwrap();
        Terminal::negotiate(stream).await.unwrap()
    }

    #[tokio::test]
    async fn negotiate_uses_query_reply_size() {
        let term = negotiate_with(true).await;
        assert_eq!((term.rows, term.cols), (43, 80));
    }

    #[tokio::test]
    async fn negotiate_falls_back_to_terminal_type() {
        let term = negotiate_with(false).await;
        assert_eq!((term.rows, term.cols), (24, 80));
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    #[test]
    fn cursor_address_is_parsed() {
        // AID (Enter=0x7D), Cursor 12-Bit auf Position 80 (Zeile 1, Spalte 0)
        let rec = [0x7D, ADDR_TABLE[1], ADDR_TABLE[16], ORDER_SBA];
        let inb = parse_inbound(&rec);
        assert_eq!(inb.cursor, Some(80));
    }
}
