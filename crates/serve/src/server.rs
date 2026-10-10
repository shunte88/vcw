/*
 *  server.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The HTTP listener itself: static frontend, one command endpoint, one event stream (§52).
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

//! The listener.
//!
//! Three things are served and there is deliberately no fourth: the built
//! frontend as static files, `POST /command/<name>` carrying one command's
//! JSON arguments, and `GET /events` as the single event stream. Between them
//! that is the whole of `app/ui/src/api.ts`, which is 37 functions over
//! `invoke` and one `listen`. §52 forbids adding a command the desktop shell
//! does not have, so this surface cannot grow without that one growing first.
//!
//! Thread per connection, because this tree has no async runtime and is not
//! getting one. That is affordable at the scale §52 describes: one operator,
//! one record, one browser tab.

use std::io::{self, Read, Write};
use std::net::ToSocketAddrs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use tiny_http::{Header, Request, Response, Server, StatusCode};

use crate::auth::{Guard, Verdict};

/// What the transport calls to get work done.
///
/// One method per half of `api.ts`, and no more, because this is the seam §2
/// exists to protect: anything that decides something belongs behind it, and
/// anything in front of it is transport. A desktop shell and a browser must
/// come out the same way, so neither of these may grow a parameter that only
/// one caller can supply.
pub trait Backend: Send + Sync + 'static {
    /// Run one command. `name` is the `invoke` name the frontend already uses
    /// and `arguments` is its JSON payload; the answer is JSON too.
    ///
    /// **The error is a serialized `Failure`, not a bare message.** That is
    /// exactly what `invoke` rejects with in the desktop shell, and sending
    /// anything else here would make `asFailure` read the two transports
    /// differently - which is the one thing §52 is trying to avoid. Refusing
    /// is an ordinary answer, here as everywhere else in VCW.
    fn call(&self, name: &str, arguments: &str) -> Result<String, String>;

    /// Subscribe to the one event stream, as JSON-encoded `Wire` values.
    ///
    /// One receiver per connected browser. Dropping it unsubscribes, which is
    /// what a closed tab looks like from here.
    fn subscribe(&self) -> Receiver<String>;
}

/// How long a quiet event stream waits before sending a comment line.
///
/// Not for the browser, which is patient, but for anything between it and us
/// that reaps an idle socket. Fifteen seconds is under every default we know
/// of and costs nine bytes a minute.
const HEARTBEAT: Duration = Duration::from_secs(15);

/// A running listener.
pub struct Listener {
    server: Server,
    guard: Arc<Guard>,
    root: PathBuf,
    backend: Arc<dyn Backend>,
}

impl Listener {
    /// Bind, but do not start answering.
    ///
    /// `root` is the directory holding the built frontend. It is canonicalized
    /// here so that every later path check compares resolved paths, which is
    /// the only comparison that means anything once symlinks exist.
    pub fn bind(
        address: &str,
        guard: Guard,
        root: &Path,
        backend: Arc<dyn Backend>,
    ) -> io::Result<Self> {
        let resolved = root.canonicalize().map_err(|why| {
            io::Error::new(
                why.kind(),
                format!("the frontend is not readable at {}: {why}", root.display()),
            )
        })?;
        let socket = address
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::other(format!("{address:?} is not an address")))?;
        let server = Server::http(socket).map_err(io::Error::other)?;
        Ok(Self {
            server,
            guard: Arc::new(guard),
            root: resolved,
            backend,
        })
    }

    /// The address actually bound, which is what the banner must print when
    /// the operator asked for port 0.
    pub fn address(&self) -> String {
        self.server
            .server_addr()
            .to_ip()
            .map_or_else(|| "?".to_owned(), |at| at.to_string())
    }

    /// Answer requests until the process ends.
    pub fn serve(self) -> ! {
        let server = Arc::new(self.server);
        loop {
            let Ok(request) = server.recv() else { continue };
            let guard = Arc::clone(&self.guard);
            let backend = Arc::clone(&self.backend);
            let root = self.root.clone();
            // ponytail: thread per connection, unbounded. One operator and one
            // tab is the scale §52 describes; a pool belongs here the day
            // `serve` is asked to face more than one person.
            std::thread::spawn(move || {
                if let Err(why) = answer(request, &guard, &root, backend.as_ref()) {
                    tracing::debug!(%why, "a connection ended early");
                }
            });
        }
    }
}

/// Decide and answer one request.
fn answer(
    mut request: Request,
    guard: &Guard,
    root: &Path,
    backend: &dyn Backend,
) -> io::Result<()> {
    let (path, query) = split_query(request.url());
    let (path, query) = (path.to_owned(), query.map(str::to_owned));
    let host = header(&request, "host");
    let cookie = header(&request, "cookie");

    let remember = match guard.admit(host.as_deref(), cookie.as_deref(), query.as_deref()) {
        Verdict::Admit => false,
        Verdict::AdmitAndRemember => true,
        Verdict::Refuse(reason) => {
            // 403 and not 401: there is no challenge a browser could usefully
            // answer, and a `WWW-Authenticate` would put a password box in
            // front of a person whose credential is in a URL.
            tracing::warn!(%reason, "refused a request");
            return request.respond(text(403, &reason));
        }
    };

    // A token that arrived in the URL is taken out of it immediately: a
    // redirect to the same path without the query means it never reaches
    // browser history, and the cookie carries it from here on.
    if remember && request.method() == &tiny_http::Method::Get && !is_api(&path) {
        let response = Response::empty(StatusCode(303))
            .with_header(set_cookie(guard))
            .with_header(location(&path));
        return request.respond(response);
    }

    let response = if path == "/events" {
        return stream_events(request, backend, remember.then_some(guard));
    } else if let Some(name) = path.strip_prefix("/command/") {
        let name = name.to_owned();
        let mut arguments = String::new();
        request.as_reader().read_to_string(&mut arguments)?;
        match backend.call(&name, &arguments) {
            Ok(answer) => json(200, &answer),
            // 422 rather than 500: a refusal is an answer, and the frontend
            // reads the body either way. A 5xx would have the browser's own
            // console calling it an error when it is a message. The body is
            // the `Failure` verbatim - see `Backend::call`.
            Err(refusal) => json(422, &refusal),
        }
    } else {
        file(root, &path)?
    };

    request.respond(if remember {
        response.with_header(set_cookie(guard))
    } else {
        response
    })
}

/// Hold the connection open and write events as they happen.
///
/// Written straight onto the socket rather than through a `Response`, because
/// a `Response` body is copied through a chunked encoder that buffers: an
/// event handed to it sits there until the buffer fills or the response ends,
/// and for a stream that never ends neither happens. Taking the writer is
/// what lets every event be followed by a flush, which is the whole contract
/// of an event stream.
///
/// No `Content-Length` and `Connection: close`, so the body runs to the end
/// of the socket. `EventSource` reconnects by itself when it gets there.
fn stream_events(
    request: Request,
    backend: &dyn Backend,
    remember: Option<&Guard>,
) -> io::Result<()> {
    let events = backend.subscribe();
    let cookie = remember.map_or_else(String::new, |guard| {
        format!("Set-Cookie: {}\r\n", guard.cookie())
    });
    let mut socket = request.into_writer();
    write!(
        socket,
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/event-stream\r\n\
         Cache-Control: no-cache\r\n\
         X-Accel-Buffering: no\r\n\
         Connection: close\r\n\
         {cookie}\r\n"
    )?;
    socket.flush()?;

    loop {
        match events.recv_timeout(HEARTBEAT) {
            Ok(event) => write!(socket, "data: {event}\n\n")?,
            // A comment line. The browser ignores it; anything between us and
            // the browser that reaps an idle socket does not.
            Err(RecvTimeoutError::Timeout) => socket.write_all(b": ping\n\n")?,
            // The publisher has gone, so the stream is over rather than
            // stalled. Closing is what tells the browser to reconnect.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        // The flush is the point. Without it an event is written into a
        // buffer and the browser waits for something that has already
        // happened.
        socket.flush()?;
    }
}

/// Anything this crate sends back, boxed so a streamed file and a JSON string
/// can be returned from the same `match` without the file being read into
/// memory first.
type Body = Response<Box<dyn Read + Send>>;

/// Serve one file out of the built frontend.
///
/// Everything that is not a file is `index.html`, because the frontend routes
/// in the browser and a reload of a deep link must not 404.
fn file(root: &Path, path: &str) -> io::Result<Body> {
    let wanted = resolve(root, path).unwrap_or_else(|| root.join("index.html"));
    let handle = std::fs::File::open(&wanted)?;
    let length = handle.metadata().ok().map(|at| at.len() as usize);
    let kind = mime(&wanted);
    Ok(Response::new(
        StatusCode(200),
        vec![parse("Content-Type", kind)],
        Box::new(handle) as Box<dyn Read + Send>,
        length,
        None,
    ))
}

/// Turn a request path into a readable file inside `root`, or nothing.
///
/// Path traversal is the whole job here. `..` is rejected before the join
/// rather than cleaned up after it, and the result is canonicalized and
/// checked to still be under `root`, which is what catches a symlink pointing
/// out of the tree - the case a textual check cannot see.
fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path.strip_prefix('/')?);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let candidate = root.join(relative).canonicalize().ok()?;
    (candidate.starts_with(root) && candidate.is_file()).then_some(candidate)
}

/// Content type by extension. Short on purpose: these are the only kinds of
/// file `vite build` puts in `dist`, and guessing at others would be inventing
/// a MIME database nothing asks for.
fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|kind| kind.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// Is this one of ours rather than a document? Decides whether a token in the
/// URL is worth a redirect, which only makes sense for something a person is
/// looking at.
fn is_api(path: &str) -> bool {
    path == "/events" || path.starts_with("/command/")
}

/// The cookie the browser holds the secret in from here on. Spelled by the
/// guard, which owns every rendering of the secret.
fn set_cookie(guard: &Guard) -> Header {
    parse("Set-Cookie", &guard.cookie())
}

fn location(path: &str) -> Header {
    parse("Location", path)
}

fn text(status: u16, body: &str) -> Body {
    wrap(status, "text/plain; charset=utf-8", body)
}

fn json(status: u16, body: &str) -> Body {
    wrap(status, "application/json", body)
}

fn wrap(status: u16, kind: &str, body: &str) -> Body {
    let bytes = body.as_bytes().to_vec();
    let length = bytes.len();
    Response::new(
        StatusCode(status),
        vec![parse("Content-Type", kind)],
        Box::new(io::Cursor::new(bytes)) as Box<dyn Read + Send>,
        Some(length),
        None,
    )
}

/// A header from two strings we wrote ourselves.
fn parse(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("a header this crate spelled itself")
}

fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|held| held.field.equiv(name))
        .map(|held| held.value.as_str().to_owned())
}

fn split_query(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    }
}
