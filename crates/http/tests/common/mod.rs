//! A small HTTP server on localhost for the adapter's tests.

#![allow(clippy::expect_used, reason = "fixture helpers outside #[test] fns")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

/// Serves `body` with `status` to `requests` connections, honouring a `Range` header.
pub fn serve(body: Vec<u8>, status: u16, requests: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("an address");
    thread::spawn(move || {
        for stream in listener.incoming().take(requests) {
            let mut stream = stream.expect("a connection");
            let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
            let mut range = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("a header line");
                if line.trim().is_empty() {
                    break;
                }
                if let Some(spec) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    let (start, end) = spec.trim().split_once('-').expect("a range");
                    range = Some((
                        start.parse::<usize>().expect("a start"),
                        end.parse::<usize>().expect("an end"),
                    ));
                }
            }
            let head = match (status, range) {
                (200, Some((start, end))) => {
                    let end = end.min(body.len() - 1);
                    let length = end + 1 - start;
                    let total = body.len();
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {length}\r\n\
                         Content-Range: bytes {start}-{end}/{total}\r\nConnection: close\r\n\r\n"
                    )
                    .into_bytes()
                    .into_iter()
                    .chain(body[start..=end].iter().copied())
                    .collect::<Vec<u8>>()
                }
                (200, None) => format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes()
                .into_iter()
                .chain(body.iter().copied())
                .collect(),
                _ => format!(
                    "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .into_bytes(),
            };
            let _ = stream.write_all(&head);
        }
    });
    format!("http://{address}")
}
