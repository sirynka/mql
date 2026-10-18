// Sirynka: very rudimentary and almost 100% vibe-codded

mod render;
mod server;

use mql::mql::MQL;
use mql::song::{Songs, anyhow_songs_to_ok_err, read_songs_in_dir};
use server::{lan_ip, query_param, read_request, respond, url_decode};

use std::net::{TcpListener, TcpStream};
use itertools::Itertools;

fn main() -> anyhow::Result<()> {
    let songs = read_songs_in_dir("/home/sirynka/Music/Liked/")?;
    let (songs, _) = anyhow_songs_to_ok_err(songs);
    println!("{} songs", songs.0.len());

    let listener = TcpListener::bind("0.0.0.0:8080")?;
    let port = listener.local_addr()?.port();

    if let Some(ip) = lan_ip() { println!("http://{ip}:{port}"); }
    println!("http://127.0.0.1:{port}");

    for stream in listener.incoming() {
        if let Err(e) = stream.and_then(|s| handle_response(s, &songs)) {
            eprintln!("{e}");
        }
    }

    Ok(())
}

fn handle_response(mut stream: TcpStream, songs: &Songs) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream)? else { return Ok(()); };

    let query = |path: &str| match query_param(path, "q").and_then(url_decode) {
        None => ("400 Bad Request", "bad query".to_string()),
        Some(q) => match run_query(&q, songs) {
            Ok(html)  => ("200 OK", html),
            Err(html) => ("422 Unprocessable Content", html),
        },
    };

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/")          => respond(&mut stream, "200 OK", "text/html",       &std::fs::read_to_string("web-ui/index.html")?),
        ("GET", "/style.css") => respond(&mut stream, "200 OK", "text/css",        &std::fs::read_to_string("web-ui/style.css")?),
        ("GET", "/main.js")   => respond(&mut stream, "200 OK", "text/javascript", &std::fs::read_to_string("web-ui/main.js")?),

        ("GET", path) if path.starts_with("/q?") => {
            let (status, html) = query(path);
            respond(&mut stream, status, "text/html", &html)
        },

        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

// Claude: `Err` is a parse error, also rendered as HTML
fn run_query(q: &str, songs: &Songs) -> Result<String, String> {
    let ast = MQL::parse(q).map_err(|e| render::error(&e).into_string())?;
    let matched = songs.0.iter().filter(|s| s.matches(&ast)).collect_vec();
    Ok(render::songs(&matched).into_string())
}
