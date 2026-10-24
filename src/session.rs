//! Sitzungen: Navigationsstack aus Views, Eingabe-Zuordnung und Server-Schleife.
//!
//! Jede TCP-Verbindung ist eine eigene Sitzung mit eigenem Stack und eigenem
//! Datenkontext. Der geteilte Zustand `S` (z. B. ein OData-Client oder der
//! Pfad zu einer Datenbank) wird pro Verbindung geklont.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
#[cfg(feature = "tls")]
use anyhow::Context as _;
use async_trait::async_trait;
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
#[cfg(feature = "tls")]
use tokio_rustls::TlsAcceptor;
use tokio::time::{Instant, MissedTickBehavior, interval_at};
use tracing::{info, warn};

use crate::context::Context;
use crate::screen::{FieldId, InputField, Patch, Screen};
use crate::tn3270::{Aid, Terminal, parse_inbound};

/// Was die Schleife aufweckt: eine Eingabe oder der Timer eines Views.
enum Wake {
    /// Eingabe-Record, `None` = Verbindung geschlossen.
    Input(Option<Vec<u8>>),
    Tick,
}

/// Was nach einer Taste passieren soll.
pub enum Action {
    /// Auf dem aktuellen View bleiben.
    Stay,
    /// Neuen View mit Uebergabeparametern auf den Stack legen.
    Open { view: &'static str, params: Value },
    /// Zum vorherigen View zurueckkehren. Im Startview beendet es die Sitzung.
    Back,
    /// Sitzung beenden.
    Exit,
    /// Abschlussmeldung auf leerem Bildschirm zeigen, dann sofort beenden
    /// (ohne auf eine Taste zu warten).
    Farewell(String),
}

/// Zustand einer Sitzung, den alle Views gemeinsam nutzen.
pub struct App<S> {
    /// Vom Server geklonter, geteilter Zustand (z. B. Client fuer einen Dienst).
    pub shared: S,
    /// Sitzungsdaten als JSON-Pointer-Baum (siehe [`Context`]).
    pub ctx: Context,
    /// Meldungszeile, wird vom View gerendert.
    pub msg: String,
    pub rows: u16,
    pub cols: u16,
}

/// Ein Bildschirm mit Darstellung und Tastenbehandlung.
#[async_trait]
pub trait View<S: Send + 'static>: Send {
    /// Wird beim Oeffnen aufgerufen. Ein Fehler verhindert das Oeffnen und
    /// wird als Meldung angezeigt.
    async fn enter(&mut self, _app: &mut App<S>, _params: Value) -> Result<(), String> {
        Ok(())
    }

    /// Baut den Bildschirm aus dem aktuellen Zustand.
    fn render(&self, app: &App<S>) -> Screen;

    /// Verarbeitet eine AID-Taste mit den geaenderten Eingabefeldern.
    async fn handle(&mut self, app: &mut App<S>, aid: Aid, values: &[(FieldId, String)]) -> Action;

    /// Intervall fuer unangeforderte Aktualisierung (Server Push). `None` =
    /// der View wird nur nach Eingaben neu gezeichnet.
    fn refresh_interval(&self) -> Option<Duration> {
        None
    }

    /// Wird im Intervall von [`View::refresh_interval`] aufgerufen. Liefert
    /// eine Teilaktualisierung oder `None`, wenn nichts zu tun ist.
    fn refresh(&self, _app: &App<S>) -> Option<Patch> {
        None
    }
}

/// Ordnet die Antwortfelder des Terminals den Eingabefeldern zu. Die Antwort
/// liefert die Position des Feld-Datenbeginns (Attribut + 1).
pub fn map_fields(inputs: &[InputField], fields: &[(usize, String)]) -> Vec<(FieldId, String)> {
    fields
        .iter()
        .filter_map(|(pos, text)| {
            inputs
                .iter()
                .find(|f| *pos > f.pos && *pos <= f.pos + f.len as usize)
                .map(|f| (f.id, text.trim().to_string()))
        })
        .collect()
}

type Factory<S> = Box<dyn Fn() -> Box<dyn View<S>> + Send + Sync>;

/// Ermoeglicht `Server::view(name, || -> Box<dyn View<S>> { ... })` ohne
/// eigenen Newtype. Alle Methoden werden an den inneren View weitergereicht.
#[async_trait]
impl<S: Send + 'static> View<S> for Box<dyn View<S>> {
    async fn enter(&mut self, app: &mut App<S>, params: Value) -> Result<(), String> {
        (**self).enter(app, params).await
    }

    fn render(&self, app: &App<S>) -> Screen {
        (**self).render(app)
    }

    async fn handle(&mut self, app: &mut App<S>, aid: Aid, values: &[(FieldId, String)]) -> Action {
        (**self).handle(app, aid, values).await
    }

    fn refresh_interval(&self) -> Option<Duration> {
        (**self).refresh_interval()
    }

    fn refresh(&self, app: &App<S>) -> Option<Patch> {
        (**self).refresh(app)
    }
}

/// Server mit registrierten Views. Jede Verbindung startet am Startview.
pub struct Server<S> {
    views: HashMap<&'static str, Factory<S>>,
    start: &'static str,
    shared: S,
    #[cfg(feature = "tls")]
    tls: Option<TlsAcceptor>,
}

impl<S> Server<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// `start` ist der Name des Startviews.
    pub fn new(shared: S, start: &'static str) -> Self {
        Self {
            views: HashMap::new(),
            start,
            shared,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }

    /// Schaltet TLS ein: Zertifikatskette und Schluessel als PEM-Dateien.
    /// Ohne diesen Aufruf laeuft der Server im Klartext.
    #[cfg(feature = "tls")]
    pub fn with_tls(
        mut self,
        cert: impl AsRef<std::path::Path>,
        key: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        self.tls = Some(crate::tls::acceptor(cert.as_ref(), key.as_ref(), None)?);
        Ok(self)
    }

    /// Wie [`Server::with_tls`], verlangt zusaetzlich ein Client-Zertifikat
    /// von der CA in `client_ca`.
    #[cfg(feature = "tls")]
    pub fn with_tls_client_auth(
        mut self,
        cert: impl AsRef<std::path::Path>,
        key: impl AsRef<std::path::Path>,
        client_ca: impl AsRef<std::path::Path>,
    ) -> Result<Self> {
        self.tls = Some(crate::tls::acceptor(
            cert.as_ref(),
            key.as_ref(),
            Some(client_ca.as_ref()),
        )?);
        Ok(self)
    }

    /// Registriert einen View unter `name`. Pro Oeffnen entsteht eine neue Instanz.
    pub fn view<V>(
        mut self,
        name: &'static str,
        make: impl Fn() -> V + Send + Sync + 'static,
    ) -> Self
    where
        V: View<S> + 'static,
    {
        self.views
            .insert(name, Box::new(move || Box::new(make()) as Box<dyn View<S>>));
        self
    }

    /// Nimmt Verbindungen an und bedient jede in einer eigenen Task.
    pub async fn serve(self, addr: impl ToSocketAddrs) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        info!(addr = ?listener.local_addr()?, "3270-Server laeuft");
        let server = Arc::new(self);
        loop {
            let (stream, peer) = listener.accept().await?;
            let server = server.clone();
            tokio::spawn(async move {
                info!(%peer, "Verbindung");
                if let Err(e) = server.accept_and_run(stream).await {
                    warn!(%peer, error = %format!("{e:#}"), "Sitzung mit Fehler beendet");
                }
                info!(%peer, "Verbindung beendet");
            });
        }
    }

    fn create(&self, name: &str) -> Option<Box<dyn View<S>>> {
        self.views.get(name).map(|make| make())
    }

    /// Baut bei TLS zuerst den verschluesselten Kanal auf, danach die Sitzung.
    async fn accept_and_run(&self, stream: TcpStream) -> Result<()> {
        #[cfg(feature = "tls")]
        if let Some(acceptor) = &self.tls {
            let tls = acceptor.accept(stream).await.context("TLS-Handshake")?;
            return self.run_session(tls).await;
        }
        self.run_session(stream).await
    }

    async fn run_session<T: crate::tn3270::Transport + 'static>(&self, stream: T) -> Result<()> {
        let mut term = Terminal::negotiate(stream).await?;
        info!(rows = term.rows, cols = term.cols, "Terminal ausgehandelt");

        let mut app = App {
            shared: self.shared.clone(),
            ctx: Context::default(),
            msg: String::new(),
            rows: term.rows,
            cols: term.cols,
        };
        let mut root = self
            .create(self.start)
            .ok_or_else(|| anyhow!("Startview '{}' nicht registriert", self.start))?;
        if let Err(e) = root.enter(&mut app, Value::Null).await {
            app.msg = e;
        }
        let mut stack: Vec<Box<dyn View<S>>> = vec![root];

        loop {
            let Some(top) = stack.last_mut() else {
                break;
            };
            // Vollstaendiger Neuaufbau (Erase/Write) nach jedem Ereignis.
            let screen = top.render(&app);
            term.write_record(&screen.to_stream().to_bytes()).await?;

            let mut ticker = top.refresh_interval().map(|period| {
                let mut t = interval_at(Instant::now() + period, period);
                t.set_missed_tick_behavior(MissedTickBehavior::Skip);
                t
            });

            // Auf Eingabe oder Timer warten. Teilaktualisierungen werden direkt
            // gesendet, ohne den Benutzer zu unterbrechen.
            let rec = loop {
                let wake = match ticker.as_mut() {
                    Some(t) => tokio::select! {
                        rec = term.read_record() => Wake::Input(rec?),
                        _ = t.tick() => Wake::Tick,
                    },
                    None => Wake::Input(term.read_record().await?),
                };
                match wake {
                    Wake::Input(rec) => break rec,
                    Wake::Tick => {
                        if let Some(patch) = top.refresh(&app).filter(|p| !p.is_empty()) {
                            term.write_record(&patch.to_stream(term.cols).to_bytes())
                                .await?;
                        }
                    }
                }
            };
            let Some(rec) = rec else {
                info!("Verbindung durch Client beendet");
                break;
            };
            let inbound = parse_inbound(&rec);
            let values = map_fields(&screen.inputs, &inbound.fields);
            let action = top.handle(&mut app, inbound.aid, &values).await;

            match action {
                Action::Stay => {}
                Action::Open { view, params } => match self.create(view) {
                    Some(mut next) => match next.enter(&mut app, params).await {
                        Ok(()) => stack.push(next),
                        Err(e) => app.msg = e,
                    },
                    None => app.msg = format!("Unbekannter Screen: {view}"),
                },
                Action::Back => {
                    if stack.len() > 1 {
                        stack.pop();
                    } else {
                        break;
                    }
                }
                Action::Exit => {
                    // Bildschirm leeren, damit der Client keine alte Maske stehen laesst.
                    let blank = Screen::new(term.rows, term.cols).to_stream().to_bytes();
                    term.write_record(&blank).await.ok();
                    break;
                }
                Action::Farewell(msg) => {
                    let mut s = Screen::new(term.rows, term.cols);
                    s.text(3, 4, &msg, crate::FieldAttribute::normal(), Some(crate::Color::Blue));
                    term.write_record(&s.to_stream().to_bytes()).await.ok();
                    break;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{FieldId, Screen};

    fn screen_with_inputs() -> Screen {
        let mut s = Screen::new(24, 80);
        s.input(FieldId::Select(0), 3, 0, 1, "");
        s.input(FieldId::Command, 21, 11, 40, "");
        s
    }

    #[test]
    fn response_fields_map_to_inputs_by_data_position() {
        let s = screen_with_inputs();
        // Select-Feld: Attribut bei 3*80+0 = 240, Daten bei 241
        // Befehlszeile: Attribut bei 21*80+11 = 1691, Daten ab 1692
        let fields = vec![(241, "x".to_string()), (1692, " /film ".to_string())];
        let values = map_fields(&s.inputs, &fields);
        assert_eq!(
            values,
            vec![
                (FieldId::Select(0), "x".to_string()),
                (FieldId::Command, "/film".to_string()),
            ]
        );
    }

    #[test]
    fn attribute_position_is_not_an_input_value() {
        let s = screen_with_inputs();
        assert!(map_fields(&s.inputs, &[(240, "x".to_string())]).is_empty());
    }
}
