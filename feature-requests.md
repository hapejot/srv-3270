# srv-3270: Feature-Requests

Stand: Commit `7631b6323ecfaaf6cc9042e00ef767196f70ac98` (main). Die Bibliothek
wurde fuer dieses Projekt nicht veraendert; Wuensche werden hier gesammelt.

## 1. Dynamische View-Registrierung

`Server::view` nimmt einen konkreten Typ `V: View<S>`. Erweiterungen liefern
aber Fabriken zur Laufzeit (`Box<dyn View<S>>`). Derzeit ist dafuer ein
Newtype `DynView` im fiori-server noetig (`fiori-server/src/term3270.rs`).

Wunsch: `Server::view_boxed(name, Box<dyn Fn() -> Box<dyn View<S>> + Send + Sync>)`
oder `impl View<S> for Box<dyn View<S>>`. Dann entfaellt der Newtype.

> **Status: umgesetzt** (Commit `9605297`): `impl View<S> for Box<dyn View<S>>` in `src/session.rs`.
> Der Newtype `DynView` existiert im fiori-server nicht (`src/term3270.rs` fehlt),
> dort ist also nichts zu entfernen.

## 2. Hilfsfunktionen fuer Funktionstasten

Der Aid-Wert wird per Pattern (`Aid::Pf(3)`) verglichen. Eine Konstante oder
Hilfsfunktion wie `Aid::from_pf(3)` und ein Hinweis, dass `Aid::Pf(n)` fuer
PF1..PF24 gilt, wuerde den Code lesbarer machen.

> **Status: umgesetzt** (Commit `9605297`): `Aid::from_pf(n) -> Option<Aid>` in `src/tn3270.rs`, gibt `None` fuer n ausserhalb 1..=24.

## 3. Bildschirmgroesse der Ansicht

`Screen::text` kuerzt stillschweigend auf die Zeilenbreite. Fuer Listen waere
eine Hilfsfunktion `Screen::lines(top, labelled_pairs)` nuetzlich, die Label-
und Wertspalte ausrichtet und Zeilenende beachtet.

> **Status: umgesetzt** (Commit `9605297`): `Screen::lines(top, pairs)` in `src/screen.rs`. Die Hilfsfunktion nimmt `(&str, &str)`-Paare, richtet die Werte nach der laengsten Beschriftung aus und kuerzt am Zeilenende.

## 4. Unangeforderte Aktualisierung (Server Push)

Ist-Zustand: `Server::run_session` (`src/session.rs`, Schleife ab Zeile 153)
schreibt nur nach einer Eingabe des Benutzers und blockiert danach auf
`term.read_record()`. Eine Ansicht kann dadurch nicht selbst neu zeichnen,
z. B. alle N Sekunden fuer eine Live-Statusseite.

Das 3270-Protokoll erlaubt eine unangeforderte Ausgabe des Hosts. Gewuenscht ist:

- Ein optionales Intervall pro View, z. B. `fn refresh_interval(&self) -> Option<Duration>`.
- Die Schleife wartet mit `tokio::select!` gleichzeitig auf Eingabe und Timer.
  Bei Timer: `render` aufrufen und ohne Lesen senden. Die Tastatur bleibt
  dabei benutzbar.
- Alternativ ein Callback oder Kanal, ueber den die Anwendung eine Aktualisierung anstoesst.

Ohne diese Aenderung zeigt die Log-Ansicht nur beim Druecken von Enter den neuen Stand.
Auch hilfreich wäre dieses Feature um die aktuelle Uhrzeit / Datum im Titel anzuzeigen.

> **Status: umgesetzt** (Commit `9605297`):
> - `View::refresh_interval()` und `View::refresh()` in `src/session.rs`. Die Schleife wartet mit `tokio::select!` auf Eingabe und Timer.
> - `read_record` ist cancel-safe, der Parserzustand liegt im `Terminal` (`src/tn3270.rs`).
> - Teilaktualisierung ueber `Patch` (`src/screen.rs`): `Write` ohne Erase, ohne MDT-Reset, ohne Cursor.
> - Beispiel: Uhrzeit in der Titelzeile der Listenansicht in `examples/keepass_demo.rs`, Aktualisierung jede Sekunde.
> - Offen: Live-Test mit x3270/c3270 steht noch aus.
