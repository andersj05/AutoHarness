use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::time::Duration;

use reqwest::Url;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

pub(crate) struct ResponseSpec {
    status: u16,
    content_type: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: Vec<u8>,
}

impl ResponseSpec {
    pub(crate) fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type: "application/json",
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub(crate) fn sse(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream; charset=utf-8",
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub(crate) fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }
}

pub(crate) struct RecordedRequest {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl RecordedRequest {
    pub(crate) fn method(&self) -> &str {
        &self.method
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn header_equals(&self, name: &str, expected: &str) -> bool {
        self.headers
            .get(&name.to_ascii_lowercase())
            .is_some_and(|value| value == expected)
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }
}

impl Debug for RecordedRequest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecordedRequest")
            .field("method", &self.method)
            .field("target_bytes", &self.target.len())
            .field("header_count", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

pub(crate) async fn spawn(responses: Vec<ResponseSpec>) -> (Url, JoinHandle<Vec<RecordedRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let base_url = Url::parse(&format!("http://{address}/")).expect("fixture URL");
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut socket, _) = listener.accept().await.expect("fixture request");
            requests.push(read_request(&mut socket).await);
            write_response(&mut socket, response).await;
        }
        requests
    });
    (base_url, task)
}

pub(crate) async fn spawn_slow_sse(
    initial_event: Vec<u8>,
) -> (Url, JoinHandle<(RecordedRequest, bool)>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let base_url = Url::parse(&format!("http://{address}/")).expect("fixture URL");
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("fixture request");
        let request = read_request(&mut socket).await;
        let headers = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: text/event-stream\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Connection: keep-alive\r\n",
            "\r\n"
        );
        socket.write_all(headers.as_bytes()).await.expect("headers");
        socket
            .write_all(format!("{:X}\r\n", initial_event.len()).as_bytes())
            .await
            .expect("chunk size");
        socket.write_all(&initial_event).await.expect("event");
        socket.write_all(b"\r\n").await.expect("terminator");
        socket.flush().await.expect("flush");

        let mut byte = [0_u8; 1];
        let disconnected = matches!(
            tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte)).await,
            Ok(Ok(0)) | Ok(Err(_))
        );
        (request, disconnected)
    });
    (base_url, task)
}

async fn read_request(socket: &mut TcpStream) -> RecordedRequest {
    const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            break index + 4;
        }
        assert!(bytes.len() < MAX_REQUEST_BYTES);
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk).await.expect("request");
        assert_ne!(read, 0);
        bytes.extend_from_slice(&chunk[..read]);
    };
    let text = std::str::from_utf8(&bytes[..header_end]).expect("headers UTF-8");
    let mut lines = text.split("\r\n");
    let mut request = lines.next().expect("request line").split_whitespace();
    let method = request.next().expect("method").to_owned();
    let target = request.next().expect("target").to_owned();
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').expect("header");
        headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let required = header_end.saturating_add(content_length);
    while bytes.len() < required {
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk).await.expect("body");
        assert_ne!(read, 0);
        bytes.extend_from_slice(&chunk[..read]);
    }
    RecordedRequest {
        method,
        target,
        headers,
        body: bytes[header_end..required].to_vec(),
    }
}

async fn write_response(socket: &mut TcpStream, response: ResponseSpec) {
    let reason = match response.status {
        200 => "OK",
        302 => "Found",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        _ => "Fixture",
    };
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );
    for (name, value) in response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    socket
        .write_all(headers.as_bytes())
        .await
        .expect("response headers");
    socket
        .write_all(&response.body)
        .await
        .expect("response body");
    socket.flush().await.expect("flush response");
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
