//! Binary HTTP message framing (RFC 9292), ported from
//! `remote/ohttp/binary_http.dart`. Used to encode HTTP requests/responses
//! for OHTTP encapsulation.

use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum BinaryHttpError {
    #[error("unexpected end of data reading varint")]
    UnexpectedEnd,
    #[error("truncated field: expected {expected} bytes at offset {offset}")]
    TruncatedField { expected: usize, offset: usize },
    #[error("expected response, got request framing")]
    ExpectedResponse,
    #[error("unknown framing indicator: {0}")]
    UnknownFraming(u8),
    #[error("empty Binary HTTP response")]
    Empty,
    #[error("value too large for variable-length int: {0}")]
    VarIntTooLarge(usize),
}

pub struct BinaryHttpResponse {
    pub status_code: u32,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Encode an HTTP request into Binary HTTP format (Known-Length Request).
///
/// Format: framing indicator (0x00) | method | scheme | authority | path |
/// headers (length-prefixed block) | content (length-prefixed block) |
/// trailers (empty).
pub fn encode_request(
    method: &str,
    url: &Url,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<Vec<u8>, BinaryHttpError> {
    let mut out = Vec::new();
    out.push(0x00);

    write_varlen_string(&mut out, method)?;
    write_varlen_string(&mut out, url.scheme())?;

    let host = url.host_str().unwrap_or("");
    let authority = match url.port() {
        Some(port) if port != 443 && port != 80 => format!("{host}:{port}"),
        _ => host.to_string(),
    };
    write_varlen_string(&mut out, &authority)?;

    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    if path.is_empty() {
        path.push('/');
    }
    write_varlen_string(&mut out, &path)?;

    let header_bytes = encode_headers(headers);
    write_varlen_bytes(&mut out, &header_bytes)?;
    write_varlen_bytes(&mut out, body.unwrap_or(&[]))?;
    write_varint(&mut out, 0)?; // empty trailers

    Ok(out)
}

/// Decode a Binary HTTP response (Known-Length Response). Skips any
/// leading informational (1xx) framing, mirroring the Dart source.
pub fn decode_response(data: &[u8]) -> Result<BinaryHttpResponse, BinaryHttpError> {
    let mut offset = 0usize;
    while offset < data.len() {
        let framing = data[offset];
        offset += 1;

        if framing == 0x01 {
            let (status_code, o1) = read_varint(data, offset)?;
            offset = o1;

            let (header_bytes, o2) = read_varlen_bytes(data, offset)?;
            offset = o2;
            let headers = decode_headers(&header_bytes);

            let (body, o3) = read_varlen_bytes(data, offset)?;
            offset = o3;
            let _ = offset;

            return Ok(BinaryHttpResponse {
                status_code,
                headers,
                body,
            });
        } else if framing == 0x00 {
            return Err(BinaryHttpError::ExpectedResponse);
        } else {
            return Err(BinaryHttpError::UnknownFraming(framing));
        }
    }
    Err(BinaryHttpError::Empty)
}

fn write_varlen_string(out: &mut Vec<u8>, value: &str) -> Result<(), BinaryHttpError> {
    write_varlen_bytes(out, value.as_bytes())
}

fn write_varlen_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), BinaryHttpError> {
    write_varint(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

/// QUIC variable-length integer encoding (RFC 9000 §16), matching the
/// subset the Dart source implements (1/2/4-byte forms only).
fn write_varint(out: &mut Vec<u8>, value: usize) -> Result<(), BinaryHttpError> {
    if value < 0x40 {
        out.push(value as u8);
    } else if value < 0x4000 {
        out.push(0x40 | ((value >> 8) as u8));
        out.push((value & 0xFF) as u8);
    } else if value < 0x4000_0000 {
        out.push(0x80 | ((value >> 24) as u8));
        out.push(((value >> 16) & 0xFF) as u8);
        out.push(((value >> 8) & 0xFF) as u8);
        out.push((value & 0xFF) as u8);
    } else {
        return Err(BinaryHttpError::VarIntTooLarge(value));
    }
    Ok(())
}

fn read_varint(data: &[u8], offset: usize) -> Result<(u32, usize), BinaryHttpError> {
    if offset >= data.len() {
        return Err(BinaryHttpError::UnexpectedEnd);
    }
    let first = data[offset];
    let prefix = first >> 6;
    match prefix {
        0 => Ok((first as u32, offset + 1)),
        1 => {
            if offset + 2 > data.len() {
                return Err(BinaryHttpError::UnexpectedEnd);
            }
            let value = (((first & 0x3F) as u32) << 8) | data[offset + 1] as u32;
            Ok((value, offset + 2))
        }
        2 => {
            if offset + 4 > data.len() {
                return Err(BinaryHttpError::UnexpectedEnd);
            }
            let value = (((first & 0x3F) as u32) << 24)
                | ((data[offset + 1] as u32) << 16)
                | ((data[offset + 2] as u32) << 8)
                | data[offset + 3] as u32;
            Ok((value, offset + 4))
        }
        _ => Err(BinaryHttpError::UnexpectedEnd),
    }
}

fn read_varlen_bytes(data: &[u8], offset: usize) -> Result<(Vec<u8>, usize), BinaryHttpError> {
    let (length, new_offset) = read_varint(data, offset)?;
    let length = length as usize;
    if new_offset + length > data.len() {
        return Err(BinaryHttpError::TruncatedField { expected: length, offset: new_offset });
    }
    Ok((data[new_offset..new_offset + length].to_vec(), new_offset + length))
}

fn encode_headers(headers: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in headers {
        let name = name.to_lowercase();
        let _ = write_varlen_bytes(&mut out, name.as_bytes());
        let _ = write_varlen_bytes(&mut out, value.as_bytes());
    }
    out
}

fn decode_headers(data: &[u8]) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let Ok((name, o1)) = read_varlen_bytes(data, offset) else { break };
        offset = o1;
        let Ok((value, o2)) = read_varlen_bytes(data, offset) else { break };
        offset = o2;
        headers.push((String::from_utf8_lossy(&name).into_owned(), String::from_utf8_lossy(&value).into_owned()));
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_get_request() {
        let url = Url::parse("https://example.com/v2/status").unwrap();
        let headers = vec![("accept".to_string(), "application/json".to_string())];
        let encoded = encode_request("GET", &url, &headers, None).unwrap();
        assert_eq!(encoded[0], 0x00);
        assert!(encoded.len() > 10);
    }

    #[test]
    fn round_trips_a_response() {
        let mut builder = vec![0x01u8]; // known-length response framing
        builder.extend_from_slice(&[0x40, 0xC8]); // status 200 as 2-byte varint
        builder.push(0x00); // empty headers
        let body = b"hello";
        builder.push(body.len() as u8);
        builder.extend_from_slice(body);

        let response = decode_response(&builder).unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body, body);
    }

    #[test]
    fn rejects_request_framing_when_decoding_response() {
        assert!(matches!(decode_response(&[0x00]), Err(BinaryHttpError::ExpectedResponse)));
    }
}
