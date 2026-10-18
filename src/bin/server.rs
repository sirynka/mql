// Very rudimentary but it works

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8080")?;
    let port = listener.local_addr()?.port();

    // To get local ip
    if let Ok(lan) = UdpSocket::bind("0.0.0.0:0").and_then(
        |s| { s.connect("1.1.1.1:80")?; s.local_addr() }
    ) {
        println!("http://{}:{port}", lan.ip());
    }

    println!("http://127.0.0.1:{port}");
    for stream in listener.incoming() {
        if let Err(e) = stream.and_then(handle) { eprintln!("{e}"); }
    }

    Ok(())
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut n = 0;
    let mut buf = [0; 8192];

    while !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
        if n == buf.len() {
            return respond(&mut stream, "431 Request Header Fields Too Large", "text/plain", "");
        }

        let read = stream.read(&mut buf[n..])?;
        if read == 0 { return Ok(()); }
        n += read;
    }

    let request = std::str::from_utf8(&buf[..n]).unwrap_or("");
    let mut request_line = request.lines().next().unwrap_or("").split(' ');
    let (method, path) = (request_line.next(), request_line.next());

    match (method, path) {
        (Some("GET"), Some("/"))          => respond(&mut stream, "200 OK", "text/html", &std::fs::read_to_string("web-ui/index.html")?),
        (Some("GET"), Some("/style.css")) => respond(&mut stream, "200 OK", "text/css",  &std::fs::read_to_string("web-ui/style.css")?),
        _                                 => respond(&mut stream, "404 Not Found", "text/plain", "not found"),
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) -> std::io::Result<()> {
    let body_len = body.len();

    let response = String::new()
        + &format!("HTTP/1.1 {status}\r\n")
        + &format!("Content-Type: {content_type}; charset=utf-8\r\n")
        + &format!("Content-Length: {body_len}\r\n")
        +         ("Connection: close\r\n")
        +         ("\r\n")
        + &format!("{body}");

    stream.write_all(response.as_bytes())
}
