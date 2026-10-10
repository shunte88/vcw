/*
 *  over_a_real_socket.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The serve listener driven over a real TCP socket, by hand-written HTTP (§52).
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

//! §52's transport, end to end.
//!
//! The requests here are written as bytes rather than built by a client,
//! deliberately. Half of what this file has to prove is that the listener
//! refuses things - a missing `Host`, a borrowed cookie under a rebound name,
//! a path climbing out of the frontend directory - and a well-behaved HTTP
//! client will not send any of them. The other half is that a browser's
//! ordinary traffic works, which the same few lines cover.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vcw_serve::auth::{Guard, Secret};
use vcw_serve::server::{Backend, Listener};

/// A backend that records what it was asked and answers from a script.
struct Stub {
    asked: Mutex<Vec<(String, String)>>,
    publish: Mutex<Vec<Sender<String>>>,
}

impl Backend for Stub {
    fn call(&self, name: &str, arguments: &str) -> Result<String, String> {
        self.asked
            .lock()
            .expect("the log")
            .push((name.to_owned(), arguments.to_owned()));
        match name {
            "about" => Ok(r#"{"version":"0.2.2"}"#.to_owned()),
            "explode" => Err(
                r#"{"code":"busy","message":"this project is open somewhere else","field":null}"#
                    .to_owned(),
            ),
            other => Err(format!(
                r#"{{"code":"not-wired","message":"{other} is not a command","field":null}}"#
            )),
        }
    }

    fn subscribe(&self) -> Receiver<String> {
        let (to, from) = channel();
        self.publish.lock().expect("the subscribers").push(to);
        from
    }
}

impl Stub {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            asked: Mutex::new(Vec::new()),
            publish: Mutex::new(Vec::new()),
        })
    }

    fn emit(&self, event: &str) {
        for to in self.publish.lock().expect("the subscribers").iter() {
            let _ = to.send(event.to_owned());
        }
    }

    fn subscribers(&self) -> usize {
        self.publish.lock().expect("the subscribers").len()
    }
}

/// A listener on a free loopback port, with a frontend of two files.
struct Running {
    port: u16,
    backend: Arc<Stub>,
    #[expect(dead_code, reason = "held so the frontend outlives the listener")]
    root: tempfile::TempDir,
}

const TOKEN: &str = "abc123";

fn start() -> Running {
    let root = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(root.path().join("index.html"), "<!doctype html>hello").expect("index");
    std::fs::create_dir(root.path().join("assets")).expect("assets");
    std::fs::write(root.path().join("assets/app.js"), "export const x = 1;").expect("js");
    // A file the frontend directory does not contain, one level up, which is
    // what the traversal test below tries to reach.
    std::fs::write(root.path().join("../secret.txt"), "not yours").ok();

    // Claim a port, then let go of it: the guard has to be told the address
    // before the listener binds, because the address is part of what it
    // checks.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("its address")
        .port();

    let backend = Stub::new();
    let guard = Guard::new(
        Secret::stated(TOKEN),
        &format!("127.0.0.1:{port}"),
        Vec::<String>::new(),
    );
    let listener = Listener::bind(
        &format!("127.0.0.1:{port}"),
        guard,
        root.path(),
        Arc::clone(&backend) as Arc<dyn Backend>,
    )
    .expect("the listener binds");
    std::thread::spawn(move || listener.serve());

    // Wait for the accept loop rather than sleeping a guessed interval.
    for _ in 0..200 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Running {
        port,
        backend,
        root,
    }
}

/// One response, as a status, the headers and whatever body arrived.
struct Answer {
    status: u16,
    headers: String,
    body: String,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .lines()
            .find(|line| {
                line.to_ascii_lowercase()
                    .starts_with(&name.to_ascii_lowercase())
            })
            .and_then(|line| line.split_once(':'))
            .map(|(_, value)| value.trim())
    }
}

/// Send these exact bytes and read until the server closes.
fn send(port: u16, raw: &str) -> Answer {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a timeout");
    socket.write_all(raw.as_bytes()).expect("write");
    let mut whole = Vec::new();
    let _ = socket.read_to_end(&mut whole);
    let whole = String::from_utf8_lossy(&whole).into_owned();
    let (head, body) = whole.split_once("\r\n\r\n").unwrap_or((whole.as_str(), ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    Answer {
        status,
        headers: head.to_owned(),
        body: body.to_owned(),
    }
}

/// A GET with the headers a browser would send once it holds the cookie.
fn get(port: u16, path: &str) -> Answer {
    send(
        port,
        &format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: vcw_token={TOKEN}\r\n\
             Connection: close\r\n\r\n"
        ),
    )
}

#[test]
fn the_frontend_is_served_to_a_browser_holding_the_token() {
    let running = start();
    let page = get(running.port, "/");
    assert_eq!(page.status, 200, "{}", page.headers);
    assert!(page.body.contains("hello"), "{:?}", page.body);
    assert_eq!(
        page.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );

    let script = get(running.port, "/assets/app.js");
    assert_eq!(script.status, 200);
    assert!(script.body.contains("export const x"));
    assert_eq!(
        script.header("Content-Type"),
        Some("text/javascript; charset=utf-8"),
        "a module served as the wrong type is refused by the browser, not by us"
    );
}

#[test]
fn nothing_is_served_without_the_token() {
    let running = start();
    let bare = send(
        running.port,
        &format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            running.port
        ),
    );
    assert_eq!(bare.status, 403, "{}", bare.headers);
    assert!(
        !bare.body.contains("hello"),
        "the page was served to an unauthenticated caller"
    );
    assert!(
        bare.body.contains("VCW_SERVE_TOKEN"),
        "a refusal has to say what to do: {:?}",
        bare.body
    );
}

#[test]
fn a_token_in_the_url_is_moved_into_a_cookie_and_out_of_the_address_bar() {
    let running = start();
    let first = send(
        running.port,
        &format!(
            "GET /?t={TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            running.port
        ),
    );
    assert_eq!(first.status, 303, "{}", first.headers);
    assert_eq!(
        first.header("Location"),
        Some("/"),
        "the redirect has to drop the query, or the token stays in history"
    );
    let cookie = first.header("Set-Cookie").expect("a cookie is handed over");
    assert!(cookie.contains(&format!("vcw_token={TOKEN}")), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(
        !cookie.contains("Secure"),
        "a Secure cookie on a plain-HTTP origin is never sent at all: {cookie}"
    );
}

#[test]
fn a_rebound_host_is_refused_though_it_carries_a_valid_cookie() {
    // The attack §52's Host check exists for, over a socket rather than in a
    // unit test: the page cannot read the cookie, but after rebinding its own
    // name the browser attaches it anyway.
    let running = start();
    let rebound = send(
        running.port,
        &format!(
            "GET / HTTP/1.1\r\nHost: evil.example\r\nCookie: vcw_token={TOKEN}\r\n\
             Connection: close\r\n\r\n"
        ),
    );
    assert_eq!(rebound.status, 403, "{}", rebound.headers);
    assert!(!rebound.body.contains("hello"));
}

#[test]
fn a_path_cannot_climb_out_of_the_frontend_directory() {
    let running = start();
    for climb in [
        "/../secret.txt",
        "/assets/../../secret.txt",
        "/./../secret.txt",
    ] {
        let answer = get(running.port, climb);
        assert!(
            !answer.body.contains("not yours"),
            "{climb} escaped the frontend directory"
        );
    }
}

#[test]
fn a_command_round_trips_and_a_refusal_comes_back_as_one() {
    let running = start();
    let body = r#"{"detail":1}"#;
    let answer = send(
        running.port,
        &format!(
            "POST /command/about HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: vcw_token={TOKEN}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            running.port,
            body.len()
        ),
    );
    assert_eq!(answer.status, 200, "{}", answer.headers);
    assert!(answer.body.contains("0.2.2"), "{:?}", answer.body);
    assert_eq!(
        running.backend.asked.lock().expect("the log").as_slice(),
        [("about".to_owned(), body.to_owned())],
        "the command name and its arguments both have to arrive intact"
    );

    let refused = send(
        running.port,
        &format!(
            "POST /command/explode HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: vcw_token={TOKEN}\r\n\
             Content-Length: 2\r\nConnection: close\r\n\r\n{{}}",
            running.port
        ),
    );
    assert_eq!(
        refused.status, 422,
        "a refusal is an answer, not a server fault: {}",
        refused.headers
    );
    assert!(
        refused.body.contains("open somewhere else"),
        "{:?}",
        refused.body
    );
    assert!(
        refused.body.contains(r#""code":"busy""#),
        "a refusal arrives as the Failure the shell would have rejected with, \
         not as a message this crate re-wrapped: {:?}",
        refused.body
    );
}

#[test]
fn an_event_reaches_a_subscribed_browser() {
    let running = start();
    let mut socket = TcpStream::connect(("127.0.0.1", running.port)).expect("connect");
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("a timeout");
    write!(
        socket,
        "GET /events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: vcw_token={TOKEN}\r\n\r\n",
        running.port
    )
    .expect("write");

    // Wait for the subscription rather than for a guessed interval, then
    // publish - an event sent before anyone is listening proves nothing.
    for _ in 0..400 {
        if running.backend.subscribers() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(running.backend.subscribers(), 1, "nobody subscribed");
    running
        .backend
        .emit(r#"{"event":"capture-progress","frames":48000}"#);

    let mut seen = String::new();
    let mut buffer = [0u8; 512];
    while !seen.contains("capture-progress") {
        let read = socket.read(&mut buffer).expect("the stream stays open");
        assert!(read > 0, "the stream ended before the event arrived");
        seen.push_str(&String::from_utf8_lossy(&buffer[..read]));
    }
    assert!(
        seen.contains("text/event-stream"),
        "the content type is what makes it an EventSource: {seen:?}"
    );
    assert!(
        seen.contains("data: {\"event\":\"capture-progress\",\"frames\":48000}"),
        "{seen:?}"
    );
}
