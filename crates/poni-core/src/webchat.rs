//! Chat messages from a browser extension (WhatsApp Web), over localhost.
//!
//! The WhatsApp Web add-on's extension (`extensions/whatsapp-web`) catches
//! the notifications WhatsApp Web makes in the browser and posts each one
//! here. Nothing is read from the page beyond what WhatsApp itself would
//! show in a notification, and nothing leaves the PC: the server listens on
//! 127.0.0.1 only, runs only while the add-on is on, and is gone when it is
//! switched off.
//!
//! A request is taken only when all of these hold, so a web page, another
//! PC or a DNS-rebinding trick can't put words on the island:
//! * it came to `127.0.0.1:<port>` or `localhost:<port>` (`Host`),
//! * its `Origin` is a browser extension's (`chrome-extension://…`; a web
//!   page can't make the browser send that),
//! * it carries `X-Oyenza-Client: wa-web` (a header no simple cross-site
//!   request may add; the CORS preflight a page would need is refused).
//!
//! The wire format is as small as it can be, so this module needs nothing
//! but `std`: `POST /chat` with a plain-text body, the sender on the first
//! line and the message after it. `GET /ping` answers 200 (the extension's
//! "is the island listening").

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The port the extension posts to.
pub const PORT: u16 = 47653;
/// The value of the `X-Oyenza-Client` header.
pub const CLIENT: &str = "wa-web";
/// The most a request may be (headers and body).
const MAX_REQUEST: usize = 16 * 1024;
/// How long a client may take to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Longest sender and message kept (characters); the island cuts them again.
const MAX_FROM: usize = 200;
const MAX_TEXT: usize = 1000;
/// More than this many messages in one second are dropped.
const MAX_PER_SECOND: u32 = 10;

/// One chat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// Who it's from (the chat's name).
    pub from: String,
    /// What it says.
    pub text: String,
}

/// What to do with one request.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Message(Message),
    Ping,
    /// Not from our extension: 403.
    Refuse,
    /// From our extension but not understood: 400.
    Bad,
}

/// Decide what to do with a whole request (`port` is the one we listen on).
pub fn judge(port: u16, request: &[u8]) -> Verdict {
    let Some(split) = find(request, b"\r\n\r\n") else { return Verdict::Bad };
    let head = String::from_utf8_lossy(&request[..split]);
    let body = &request[split + 4..];
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let header = |name: &str| -> Option<&str> {
        lines.clone().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    };
    let host_ok = header("host").is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    let origin_ok = header("origin").is_some_and(|o| o.starts_with("chrome-extension://") && o.len() > "chrome-extension://".len());
    let client_ok = header("x-oyenza-client") == Some(CLIENT);
    if !(host_ok && origin_ok && client_ok) {
        return Verdict::Refuse;
    }
    match (method, path) {
        ("GET", "/ping") => Verdict::Ping,
        ("POST", "/chat") => {
            let len = header("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(body.len());
            let body = &body[..len.min(body.len())];
            match parse_body(body) {
                Some(m) => Verdict::Message(m),
                None => Verdict::Bad,
            }
        }
        _ => Verdict::Bad,
    }
}

/// "sender\nmessage": the first line is who it's from, the rest the message.
/// None when there are no words at all.
pub fn parse_body(body: &[u8]) -> Option<Message> {
    let s = String::from_utf8_lossy(body);
    let (from, text) = match s.split_once('\n') {
        Some((f, t)) => (f, t),
        None => (&*s, ""),
    };
    let from: String = from.trim().chars().take(MAX_FROM).collect();
    let text: String = text.trim().chars().take(MAX_TEXT).collect();
    if from.is_empty() && text.is_empty() {
        None
    } else {
        Some(Message { from, text })
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read one request: the headers, then as many body bytes as they promise.
/// None when it is too big, too slow or ends early.
fn read_request(s: &mut TcpStream) -> Option<Vec<u8>> {
    let _ = s.set_read_timeout(Some(READ_TIMEOUT));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(split) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..split]).to_ascii_lowercase();
            let want = head
                .split("\r\n")
                .find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok()))
                .unwrap_or(0);
            if want > MAX_REQUEST {
                return None;
            }
            if buf.len() >= split + 4 + want {
                return Some(buf);
            }
        }
        if buf.len() > MAX_REQUEST {
            return None;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn respond(s: &mut TcpStream, status: &str, body: &str) {
    let r = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = s.write_all(r.as_bytes());
}

/// The listening server. Dropping it (or [`Server::stop`]) ends its thread.
pub struct Server {
    stop: Arc<AtomicBool>,
    addr: SocketAddr,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Listen on 127.0.0.1:`port` (0: any free port, for tests) and call
    /// `on_message` for each message, on the server's own thread.
    pub fn start(port: u16, on_message: impl Fn(Message) + Send + 'static) -> std::io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new().name("poni-webchat".into()).spawn(move || {
            let (mut second, mut count) = (Instant::now(), 0u32);
            // A blocking accept: nothing wakes this thread until a request
            // comes (or `stop` connects to it).
            for conn in listener.incoming() {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(mut s) = conn else { continue };
                let Some(req) = read_request(&mut s) else {
                    respond(&mut s, "400 Bad Request", "");
                    continue;
                };
                match judge(addr.port(), &req) {
                    Verdict::Message(m) => {
                        if second.elapsed() >= Duration::from_secs(1) {
                            (second, count) = (Instant::now(), 0);
                        }
                        count += 1;
                        if count <= MAX_PER_SECOND {
                            on_message(m);
                        }
                        respond(&mut s, "204 No Content", "");
                    }
                    Verdict::Ping => respond(&mut s, "200 OK", "ok"),
                    Verdict::Refuse => respond(&mut s, "403 Forbidden", ""),
                    Verdict::Bad => respond(&mut s, "400 Bad Request", ""),
                }
            }
        })?;
        Ok(Server { stop, addr, thread: Some(thread) })
    }

    /// The address it listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop listening and end the thread.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else { return };
        self.stop.store(true, Ordering::Relaxed);
        // Wake the blocking accept so it sees the flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        let _ = thread.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn req(method: &str, path: &str, port: u16, extra: &[(&str, &str)], body: &str) -> Vec<u8> {
        let mut s = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n", body.len());
        for (k, v) in extra {
            s.push_str(&format!("{k}: {v}\r\n"));
        }
        s.push_str("\r\n");
        s.push_str(body);
        s.into_bytes()
    }

    const GOOD: [(&str, &str); 2] = [("Origin", "chrome-extension://abcdefghijklmnop"), ("X-Oyenza-Client", CLIENT)];

    #[test]
    fn a_message_from_the_extension_is_taken() {
        let r = req("POST", "/chat", 47653, &GOOD, "Budi\nMakan yuk");
        assert_eq!(judge(47653, &r), Verdict::Message(Message { from: "Budi".into(), text: "Makan yuk".into() }));
        // Header names are not case-sensitive; localhost works too.
        let r = b"POST /chat HTTP/1.1\r\nhost: localhost:1\r\norigin: chrome-extension://x\r\nx-oyenza-client: wa-web\r\ncontent-length: 4\r\n\r\nA\nB!".to_vec();
        assert_eq!(judge(1, &r), Verdict::Message(Message { from: "A".into(), text: "B!".into() }));
    }

    #[test]
    fn everything_else_is_refused() {
        let body = "Budi\nhi";
        // A web page: its own origin, or none.
        let page = [("Origin", "https://evil.example"), ("X-Oyenza-Client", CLIENT)];
        assert_eq!(judge(5, &req("POST", "/chat", 5, &page, body)), Verdict::Refuse);
        assert_eq!(judge(5, &req("POST", "/chat", 5, &[("X-Oyenza-Client", CLIENT)], body)), Verdict::Refuse);
        // Not our header.
        assert_eq!(judge(5, &req("POST", "/chat", 5, &[("Origin", "chrome-extension://x")], body)), Verdict::Refuse);
        // A CORS preflight.
        assert_eq!(judge(5, &req("OPTIONS", "/chat", 5, &[("Origin", "https://evil.example")], "")), Verdict::Refuse);
        // The wrong host (DNS rebinding) or the wrong port.
        let r = b"POST /chat HTTP/1.1\r\nHost: evil.example:5\r\nOrigin: chrome-extension://x\r\nX-Oyenza-Client: wa-web\r\n\r\nA\nB".to_vec();
        assert_eq!(judge(5, &r), Verdict::Refuse);
        assert_eq!(judge(6, &req("POST", "/chat", 5, &GOOD, body)), Verdict::Refuse);
        // A bare "chrome-extension://" is no extension.
        let bare = [("Origin", "chrome-extension://"), ("X-Oyenza-Client", CLIENT)];
        assert_eq!(judge(5, &req("POST", "/chat", 5, &bare, body)), Verdict::Refuse);
    }

    #[test]
    fn ping_and_unknown_paths() {
        assert_eq!(judge(5, &req("GET", "/ping", 5, &GOOD, "")), Verdict::Ping);
        assert_eq!(judge(5, &req("GET", "/chat", 5, &GOOD, "")), Verdict::Bad);
        assert_eq!(judge(5, &req("POST", "/nope", 5, &GOOD, "a\nb")), Verdict::Bad);
        assert_eq!(judge(5, &req("POST", "/chat", 5, &GOOD, "  \n ")), Verdict::Bad);
        assert_eq!(judge(5, b"garbage"), Verdict::Bad);
    }

    #[test]
    fn bodies_are_split_and_cut() {
        assert_eq!(parse_body(b"Who").unwrap(), Message { from: "Who".into(), text: String::new() });
        assert_eq!(parse_body(b"A\nline one\nline two").unwrap().text, "line one\nline two");
        assert_eq!(parse_body("\u{1F600}\u{1F600}\ntext".as_bytes()).unwrap().from, "\u{1F600}\u{1F600}");
        let long = format!("{}\n{}", "n".repeat(500), "t".repeat(5000));
        let m = parse_body(long.as_bytes()).unwrap();
        assert_eq!((m.from.chars().count(), m.text.chars().count()), (MAX_FROM, MAX_TEXT));
        // Content-Length bounds the body.
        let r = b"POST /chat HTTP/1.1\r\nHost: 127.0.0.1:5\r\nOrigin: chrome-extension://x\r\nX-Oyenza-Client: wa-web\r\nContent-Length: 3\r\n\r\nA\nBcdef".to_vec();
        assert_eq!(judge(5, &r), Verdict::Message(Message { from: "A".into(), text: "B".into() }));
    }

    fn call(addr: SocketAddr, bytes: &[u8]) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(bytes).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn the_server_listens_answers_and_stops() {
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let server = Server::start(0, move |m| {
            let _ = tx.lock().unwrap().send(m);
        })
        .unwrap();
        let addr = server.addr();
        assert!(addr.ip().is_loopback(), "localhost only");
        let ok = call(addr, &req("POST", "/chat", addr.port(), &GOOD, "Siti\nHalo"));
        assert!(ok.starts_with("HTTP/1.1 204"), "{ok}");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Message { from: "Siti".into(), text: "Halo".into() });
        let no = call(addr, &req("POST", "/chat", addr.port(), &[("Origin", "https://x.example"), ("X-Oyenza-Client", CLIENT)], "A\nB"));
        assert!(no.starts_with("HTTP/1.1 403"), "{no}");
        assert!(rx.try_recv().is_err(), "a refused request says nothing");
        let ping = call(addr, &req("GET", "/ping", addr.port(), &GOOD, ""));
        assert!(ping.starts_with("HTTP/1.1 200"), "{ping}");
        // Too big a body is turned away without being read.
        let big = call(addr, &req("POST", "/chat", addr.port(), &GOOD, &"x".repeat(MAX_REQUEST + 10)));
        assert!(big.starts_with("HTTP/1.1 400"), "{big}");
        server.stop();
        assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err(), "the port is free again");
    }

    #[test]
    fn more_than_ten_a_second_are_dropped() {
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let server = Server::start(0, move |m| {
            let _ = tx.lock().unwrap().send(m);
        })
        .unwrap();
        let addr = server.addr();
        for i in 0..25 {
            call(addr, &req("POST", "/chat", addr.port(), &GOOD, &format!("A\n{i}")));
        }
        server.stop();
        let n = rx.try_iter().count();
        assert!((10..25).contains(&n), "{n}");
    }
}
