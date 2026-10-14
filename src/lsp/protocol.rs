//! LSP stdio framing: `Content-Length` headers + JSON body.

use std::io::{self, BufRead, Read, Write};

use anyhow::{Context as _, bail};

/// Encode a JSON-RPC body for LSP stdio transport.
pub fn encode_message(body: &[u8]) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body);
    out
}

/// Read one framed message from `reader`. Returns `None` on clean EOF before a header.
pub fn read_message(reader: &mut impl BufRead) -> anyhow::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).context("read LSP header line")?;
        if n == 0 {
            return if content_length.is_some() {
                bail!("EOF mid LSP headers")
            } else {
                Ok(None)
            };
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            bail!("malformed LSP header: {line}");
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            content_length = Some(
                value
                    .trim()
                    .parse()
                    .with_context(|| format!("Content-Length: {value}"))?,
            );
        }
        // Content-Type and other headers ignored.
    }
    let Some(len) = content_length else {
        bail!("LSP message missing Content-Length");
    };
    let mut body = vec![0u8; len];
    Read::read_exact(reader, &mut body).context("read LSP message body")?;
    Ok(Some(body))
}

/// Write a framed message.
pub fn write_message(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    writer.write_all(&encode_message(body))?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_framed_json() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let framed = encode_message(body);
        assert!(framed.starts_with(b"Content-Length: "));
        assert!(framed.windows(body.len()).any(|w| w == body));

        let mut cursor = Cursor::new(framed);
        let decoded = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(decoded, body);
        assert!(read_message(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn rejects_missing_content_length() {
        let mut cursor = Cursor::new(b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}");
        assert!(read_message(&mut cursor).is_err());
    }
}
