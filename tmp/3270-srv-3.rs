use rust3270::{Color, Field, Screen, negotiate_telnet, show_screen, un_negotiate_telnet};
use std::collections::HashMap;
use std::net::TcpListener;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("0.0.0.0:3270")?;
    let (mut stream, _) = listener.accept()?;
    let _dev = negotiate_telnet(&mut stream)?;

    let screen: Screen = vec![
        Field {
            row: 0,
            col: 0,
            content: "Hello, 3270!".into(),
            color: Color::WHITE,
            intense: true,
            ..Default::default()
        },
        Field {
            row: 2,
            col: 0,
            content: "Name:".into(),
            color: Color::GREEN,
            ..Default::default()
        },
        Field {
            row: 2,
            col: 6,
            write: true,
            name: "name".into(),
            color: Color::TURQUOISE,
            ..Default::default()
        },
        Field {
            row: 2,
            col: 36,
            autoskip: true,
            ..Default::default()
        },
    ];

    let resp = show_screen(&screen, &HashMap::new(), 2, 7, &mut stream)?;
    println!("AID={}  name={:?}", resp.aid, resp.values.get("name"));

    un_negotiate_telnet(&mut stream, Duration::from_secs(5))?;
    Ok(())
}
