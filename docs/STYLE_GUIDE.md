# Style Guide: Farben und Hervorhebung (3270)

Dieser Guide legt fest, welche Farbe und Hervorhebung für welchen Zweck
verwendet wird. Ziel ist, dass alle Masken gleich aussehen und dieselbe
Farbe immer dieselbe Bedeutung hat.

## Grundregeln

- **Hintergrund:** immer schwarz (Standard). Keine Hintergrundfarben setzen.
- **Farbe ist Zusatzinformation:** Die Bedeutung muss auch ohne Farbe
  erkennbar sein (Text, Position, Intensität, Unterstreichung).
- **Wenig Farben pro Bildschirm:** möglichst 3 bis 4 gleichzeitig.
- **Konsistenz:** eine Farbe hat in allen Masken dieselbe Bedeutung.
- **Nur Basisfarben für Bedeutung:** Blau, Rot, Pink, Grün, Türkis, Gelb,
  Weiß. Erweiterte Farben nur für optische Verzierung.

## Zuordnung

| Zweck                                  | Farbe                    | Attribute                  | Code                                 |
|----------------------------------------|--------------------------|----------------------------|--------------------------------------|
| Titelzeile, Maskenkopf                 | Blau                     | intensiviert               | `Color::Blue`                        |
| Feldbezeichnungen, Labels              | Blau                     | normal                     | `Color::Blue`                        |
| Befehlszeile (`Befehl ==>`)            | Blau                     | normal                     | `Color::Blue`                        |
| Tasten-/Hinweiszeile (PF-Tasten)       | Blau                     | normal                     | `Color::Blue`                        |
| Spaltenköpfe von Listen                | Weiß                     | intensiviert               | `Color::White`                       |
| Eingabefelder (ungeschützt)            | Türkis                   | unterstrichen              | `Color::Turquoise` + `Underscore`    |
| Pflichtfelder                          | Türkis                   | unterstrichen             | wie Eingabefelder (Kennzeichnung im Label, z. B. `*`) |
| Ausgabe- und Datenfelder (geschützt)   | Grün                     | normal                     | `Color::Green`                       |
| Listeneinträge                         | Grün                     | normal                     | `Color::Green`                       |
| Meldungszeile (allgemein)              | Gelb                     | intensiviert               | `Color::Yellow`                      |
| Fehlermeldung                          | Rot                      | intensiviert               | `Color::Red`                         |
| Erfolgsmeldung                         | Grün                     | intensiviert               | `Color::Green`                       |
| Hilfe, Kontextinfo                     | Pink                     | normal                     | `Color::Pink`                        |
| Cursor-/Auswahlzeile                   | Weiß                     | Reverse-Video              | `Color::White` + `Highlight::ReverseVideo` |

## Eingabefelder

Eingabefelder sind immer **türkis und unterstrichen**. So erkennt der
Benutzer sofort, wo er tippen kann, auch wenn das Feld leer ist.
Umgesetzt in `Screen::input_with` (`src/screen.rs`), das die Farbe und
Unterstreichung automatisch setzt und danach zurücksetzt.

Ein Eingabefeld, das nur einen Wert anzeigt, ohne bearbeitbar zu sein,
ist **kein** Eingabefeld, sondern ein Datenfeld (grün).

## Beispiel

```rust
// Titel
s.text(0, 0, "VIDEO-STORE  Detail", FieldAttribute::protected_intensified(), Some(Color::Blue));

// Label und Wert
s.text(6, 2, "Dauer", FieldAttribute::normal(), Some(Color::Blue));
s.text(6, 20, "01:32:10", FieldAttribute::normal(), Some(Color::Green));

// Eingabefeld (türkis, unterstrichen automatisch)
s.input(FieldId::Command, 22, 11, 60, "");

// Fehlermeldung
s.text(23, 0, "Passwort falsch", FieldAttribute::protected_intensified(), Some(Color::Red));
```

## Checkliste für neue Masken

- [ ] Titel blau, intensiviert
- [ ] Labels blau, Werte grün
- [ ] Alle Eingabefelder über `input` / `input_with` (türkis, unterstrichen)
- [ ] Meldungszeile gelb (Hinweis) oder rot (Fehler)
- [ ] Befehlszeile und Tastenhinweise blau
- [ ] Keine weiteren Farben ohne Begründung
