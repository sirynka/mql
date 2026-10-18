use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, UdpSocket};
use itertools::Itertools;

pub struct Request {
    pub method: String,
    pub path: String,
}

// Claude: `None` when the client hung up or the headers didn't fit (that one is already answered with 431)
pub fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut n = 0;
    let mut buf = [0; 8192];

    while !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
        if n == buf.len() {
            respond(stream, "431 Request Header Fields Too Large", "text/plain", "")?;
            return Ok(None);
        }

        let read = stream.read(&mut buf[n..])?;
        if read == 0 { return Ok(None); }
        n += read;
    }

    let Ok(request) = std::str::from_utf8(&buf[..n]) else {
        return bad_request(stream)
    };

    let line = request.lines().next().unwrap_or("");
    let [method, path, _version] = line.split(' ').collect_vec()[..] else {
        return bad_request(stream)
    };

    Ok(Some(Request {
        method: method.to_string(),
        path: path.to_string()
    }))
}

fn bad_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    respond(stream, "400 Bad Request", "text/plain", "")?;
    Ok(None)
}

pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("1.1.1.1:80").ok()?;
    Some(s.local_addr().ok()?.ip())
}

pub fn query_param<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    let (_, params) = path.split_once('?')?;
    for param in params.split('&') {
        let Some((key, value)) = param.split_once('=') else { continue };
        if key == name { return Some(value) }
    }
    None
}

// Claude: `+` is a space, `%XX` is a byte. Bytes become a String once at the end, so `%C3%B3` is `ó`
pub fn url_decode(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        bytes.push(match b {
            b'+' => b' ',
            b'%' => hex(it.next()?)? * 16 + hex(it.next()?)?,
            b => b,
        });
    }
    String::from_utf8(bytes).ok()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// Claude: `BufWriter` so the `write!`s go out as one packet, not one syscall each
pub fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) -> std::io::Result<()> {
    let mut out = std::io::BufWriter::new(stream);

    write!(out, "HTTP/1.1 {status}\r\n")?;
    write!(out, "Content-Type: {content_type}; charset=utf-8\r\n")?;
    write!(out, "Content-Length: {}\r\n", body.len())?;
    write!(out, "Connection: close\r\n")?;
    write!(out, "\r\n")?;
    write!(out, "{body}")?;

    out.flush()
}

#[test]
fn url_query() {
    assert_eq!(query_param("/q?a=1&q=x&b=2", "q"),  Some("x"));
    assert_eq!(query_param("/q?flag&q=x", "q"),     Some("x"));
    assert_eq!(query_param("/q?a=1", "q"),          None);
    assert_eq!(query_param("/q", "q"),              None);

    assert_eq!(url_decode("artist+%3D%3D+eminem"),  Some("artist == eminem".to_string()));
    assert_eq!(url_decode("%D0%A1%D0%BF%D0%BB%D0%B8%D0%BD+j%C3%B3"), Some("Сплин jó".to_string()));
    assert_eq!(url_decode("%zz"),                   None);
    assert_eq!(url_decode("%+f"),                   None);
    assert_eq!(url_decode("%4"),                    None);
    assert_eq!(url_decode("%C3"),                   None);
}
