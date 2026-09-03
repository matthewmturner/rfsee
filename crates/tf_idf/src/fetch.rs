use std::{
    io::{Read, Write},
    net::TcpStream,
};

use native_tls::TlsConnector;

use crate::{
    error::{RFSeeError, RFSeeResult},
    parse::parse_rfc_details,
    RfcEntry,
};

const RFC_INDEX_URL: &str = "https://www.rfc-editor.org/rfc-index.txt";
pub const RFC_EDITOR_URL_BASE: &str = "https://www.rfc-editor.org/rfc/rfc";

pub fn fetch(url: &str) -> RFSeeResult<String> {
    let parts: Vec<&str> = url.split("://").collect();
    if parts.len() == 2 {
        let _scheme = parts[0];
        let remaining = parts[1];
        let (domain, path) = match remaining.split_once("/") {
            Some((domain, path)) => (domain, path),
            None => {
                return Err(RFSeeError::ParseError(
                    "Unable to parse domain and path".to_string(),
                ))
            }
        };
        let path_with_prefix = format!("/{path}");
        let addr = format!("{domain}:443");

        let connector = TlsConnector::new().map_err(|e| RFSeeError::FetchError(e.to_string()))?;
        let stream =
            TcpStream::connect(&addr).map_err(|e| RFSeeError::FetchError(e.to_string()))?;
        let mut stream = connector
            .connect(domain, stream)
            .map_err(|e| RFSeeError::FetchError(e.to_string()))?;
        let req = format!(
        "GET {path_with_prefix} HTTP/1.1\r\nHost: {domain}\r\nUser-Agent: rfsee/0.0.1\r\nConnection: close\r\n\r\n"
    );
        stream
            .write_all(req.as_bytes())
            .map_err(|e| RFSeeError::IOError(e.to_string()))?;
        let mut buf = String::new();
        stream
            .read_to_string(&mut buf)
            .map_err(|e| RFSeeError::IOError(e.to_string()))?;

        Ok(buf)
    } else {
        Err(RFSeeError::ParseError(format!("Invalid URL: {url}")))
    }
}

/// Return the raw `String` contents of IETF RFC index
pub fn fetch_rfc_index() -> RFSeeResult<String> {
    let rfc_index_content = fetch(RFC_INDEX_URL)?;
    Ok(rfc_index_content)
}

pub fn fetch_rfc(raw_rfc: &str) -> RFSeeResult<RfcEntry> {
    if let Ok((rfc_num, title)) = parse_rfc_details(raw_rfc) {
        let url = format!("{RFC_EDITOR_URL_BASE}{rfc_num}.txt");
        if let Ok(content) = fetch(&url) {
            Ok(RfcEntry {
                number: rfc_num,
                url: url.clone(),
                title: title.replace("\n     ", " ").to_string(),
                content: Some(content),
            })
        } else {
            Err(RFSeeError::FetchError("Unable to fetch RFC".to_string()))
        }
    } else {
        Err(RFSeeError::ParseError(
            "Unable to parse raw RFC".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_empty_path() {
        let url = "https://www.rfc-editor.org/";
        let res = fetch(url).unwrap();
        if let Some(first_line) = res.lines().next() {
            assert_eq!(first_line, "HTTP/1.1 200 OK")
        }
    }

    #[test]
    fn fetch_rfc_index() {
        let url = RFC_INDEX_URL;
        let res = fetch(url).unwrap();
        if let Some(first_line) = res.lines().next() {
            assert_eq!(first_line, "HTTP/1.1 200 OK")
        }
    }

    #[test]
    fn fetch_rfc() {
        let url = "https://www.rfc-editor.org/rfc/rfc8124.txt";
        let res = fetch(url).unwrap();
        if let Some(first_line) = res.lines().next() {
            assert_eq!(first_line, "HTTP/1.1 200 OK")
        }
    }

    #[test]
    fn fetch_nonexistent_rfc() {
        let url = "https://www.rfc-editor.org/rfc/rfc999999999.txt";
        let res = fetch(url).unwrap();
        if let Some(first_line) = res.lines().next() {
            assert_eq!(first_line, "HTTP/1.1 404 Not Found")
        }
    }
}
