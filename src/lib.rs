//! srv-3270: Bibliothek fuer 3270-basierte Dialoge ueber TN3270 (Telnet).
//!
//! Schichten:
//! - [`tn3270`]: Telnet-Verhandlung, Records, Antwort-Parsing (Transport).
//! - [`datastream`] (intern, Teile exportiert): Encoder fuer den 3270-Datenstrom.
//! - [`screen`]: Bildschirm-Aufbau mit Textfeldern, Auswahlfeldern und Formularen.
//! - [`session`]: Views, Navigationsstack und Server.
//! - [`context`]: JSON-Datenkontext pro Sitzung.

pub mod context;
pub mod ebcdic;
mod datastream;
pub mod screen;
pub mod session;
pub mod tn3270;
#[cfg(feature = "tls")]
pub mod tls;

pub use context::Context;
pub use datastream::{
    Attribute, Color, Command, DataStream, FieldAttribute, Highlight, Order, Wcc,
    ascii_to_ebcdic, build_read_partition_query_implicit_partition, ebcdic_to_ascii_str,
    parse_implicit_partition_reply,
};
pub use screen::{Field, FieldId, FieldKind, Patch, Screen, clip, data_marker};
pub use session::{Action, App, Server, View, map_fields};
pub use tn3270::{Aid, Inbound, Terminal, parse_inbound};
