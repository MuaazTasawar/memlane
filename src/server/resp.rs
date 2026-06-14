//! RESP (REdis Serialization Protocol) parser and encoder for MemLane.
//!
//! Implements RESP2 — the wire protocol used by Redis and all Redis clients.
//! By speaking RESP, MemLane becomes a drop-in replacement: any redis-cli,
//! redis-py, ioredis, or Jedis client connects with zero code changes.
//!
//! RESP2 data types:
//!   '+' → Simple String  e.g. "+OK\r\n"
//!   '-' → Error          e.g. "-ERR unknown command\r\n"
//!   ':' → Integer        e.g. ":42\r\n"
//!   '$' → Bulk String    e.g. "$5\r\nhello\r\n"  ($-1\r\n = nil)
//!   '*' → Array          e.g. "*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n"
//!
//! Commands supported: GET, SET, DEL, EXISTS, PING, FLUSH, INFO,
//!                     MGET, MSET, INCR, DECR, TTL, COMMAND

use bytes::{Buf, BytesMut};

// ── RESP Value ────────────────────────────────────────────────────────────────

/// A parsed RESP value.
#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    /// "+OK\r\n"
    SimpleString(String),
    /// "-ERR ...\r\n"
    Error(String),
    /// ":42\r\n"
    Integer(i64),
    /// "$5\r\nhello\r\n"
    BulkString(Vec<u8>),
    /// "$-1\r\n" — nil bulk string (key not found)
    Nil,
    /// "*N\r\n ..."
    Array(Vec<RespValue>),
}

// ── Parse errors ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum RespError {
    #[error("Incomplete data — need more bytes")]
    Incomplete,
    #[error("Protocol error: {0}")]
    Protocol(String),
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Attempt to parse one complete RESP value from `buf`.
///
/// Returns `Ok(Some((value, bytes_consumed)))` on success.
/// Returns `Ok(None)` if more data is needed (incomplete frame).
/// Returns `Err(RespError)` on protocol violation.
pub fn parse(buf: &[u8]) -> Result<Option<(RespValue, usize)>, RespError> {
    if buf.is_empty() {
        return Ok(None);
    }

    match buf[0] {
        b'+' => parse_simple_string(buf),
        b'-' => parse_error(buf),
        b':' => parse_integer(buf),
        b'$' => parse_bulk_string(buf),
        b'*' => parse_array(buf),
        other => Err(RespError::Protocol(format!(
            "Unknown RESP type byte: 0x{:02x}",
            other
        ))),
    }
}

/// Parse all complete RESP values from `buf`, returning them and total bytes consumed.
pub fn parse_all(buf: &[u8]) -> Result<(Vec<RespValue>, usize), RespError> {
    let mut values = Vec::new();
    let mut offset = 0;

    loop {
        match parse(&buf[offset..])? {
            Some((val, consumed)) => {
                values.push(val);
                offset += consumed;
            }
            None => break,
        }
    }

    Ok((values, offset))
}

// ── Internal parse helpers ────────────────────────────────────────────────────

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn parse_line(buf: &[u8]) -> Result<(&str, usize), RespError> {
    match find_crlf(buf) {
        None => Err(RespError::Incomplete),
        Some(pos) => {
            let line = std::str::from_utf8(&buf[1..pos])
                .map_err(|_| RespError::Protocol("Non-UTF8 line".to_string()))?;
            Ok((line, pos + 2)) // +2 for \r\n
        }
    }
}

fn parse_simple_string(buf: &[u8]) -> Result<Option<(RespValue, usize)>, RespError> {
    match parse_line(buf) {
        Err(RespError::Incomplete) => Ok(None),
        Err(e) => Err(e),
        Ok((line, consumed)) => Ok(Some((RespValue::SimpleString(line.to_string()), consumed))),
    }
}

fn parse_error(buf: &[u8]) -> Result<Option<(RespValue, usize)>, RespError> {
    match parse_line(buf) {
        Err(RespError::Incomplete) => Ok(None),
        Err(e) => Err(e),
        Ok((line, consumed)) => Ok(Some((RespValue::Error(line.to_string()), consumed))),
    }
}

fn parse_integer(buf: &[u8]) -> Result<Option<(RespValue, usize)>, RespError> {
    match parse_line(buf) {
        Err(RespError::Incomplete) => Ok(None),
        Err(e) => Err(e),
        Ok((line, consumed)) => {
            let n: i64 = line.parse().map_err(|_| {
                RespError::Protocol(format!("Invalid integer: {}", line))
            })?;
            Ok(Some((RespValue::Integer(n), consumed)))
        }
    }
}

fn parse_bulk_string(buf: &[u8]) -> Result<Option<(RespValue, usize)>, RespError> {
    let (line, header_len) = match parse_line(buf) {
        Err(RespError::Incomplete) => return Ok(None),
        Err(e) => return Err(e),
        Ok(v) => v,
    };

    let len: i64 = line.parse().map_err(|_| {
        RespError::Protocol(format!("Invalid bulk string length: {}", line))
    })?;

    if len < 0 {
        // Nil bulk string
        return Ok(Some((RespValue::Nil, header_len)));
    }

    let len = len as usize;
    let needed = header_len + len + 2; // +2 for trailing \r\n

    if buf.len() < needed {
        return Ok(None); // Incomplete
    }

    let data = buf[header_len..header_len + len].to_vec();
    Ok(Some((RespValue::BulkString(data), needed)))
}

fn parse_array(buf: &[u8]) -> Result<Option<(RespValue, usize)>, RespError> {
    let (line, mut offset) = match parse_line(buf) {
        Err(RespError::Incomplete) => return Ok(None),
        Err(e) => return Err(e),
        Ok(v) => v,
    };

    let count: i64 = line.parse().map_err(|_| {
        RespError::Protocol(format!("Invalid array length: {}", line))
    })?;

    if count < 0 {
        return Ok(Some((RespValue::Nil, offset)));
    }

    let count = count as usize;
    let mut items = Vec::with_capacity(count);

    for _ in 0..count {
        match parse(&buf[offset..])? {
            None => return Ok(None), // Incomplete
            Some((val, consumed)) => {
                items.push(val);
                offset += consumed;
            }
        }
    }

    Ok(Some((RespValue::Array(items), offset)))
}

// ── Encoder ───────────────────────────────────────────────────────────────────

/// Encode a RespValue into bytes, appending to `buf`.
pub fn encode(val: &RespValue, buf: &mut Vec<u8>) {
    match val {
        RespValue::SimpleString(s) => {
            buf.push(b'+');
            buf.extend_from_slice(s.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::Error(e) => {
            buf.push(b'-');
            buf.extend_from_slice(e.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::Integer(n) => {
            buf.push(b':');
            buf.extend_from_slice(n.to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::BulkString(data) => {
            buf.push(b'$');
            buf.extend_from_slice(data.len().to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
            buf.extend_from_slice(data);
            buf.extend_from_slice(b"\r\n");
        }
        RespValue::Nil => {
            buf.extend_from_slice(b"$-1\r\n");
        }
        RespValue::Array(items) => {
            buf.push(b'*');
            buf.extend_from_slice(items.len().to_string().as_bytes());
            buf.extend_from_slice(b"\r\n");
            for item in items {
                encode(item, buf);
            }
        }
    }
}

/// Convenience: encode a RespValue to a fresh Vec<u8>.
pub fn encode_to_vec(val: &RespValue) -> Vec<u8> {
    let mut buf = Vec::new();
    encode(val, &mut buf);
    buf
}

// ── Command extraction ────────────────────────────────────────────────────────

/// A parsed Redis command with its arguments as raw byte vectors.
#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub args: Vec<Vec<u8>>,
}

/// Extract a Command from a parsed RESP Array value.
///
/// Redis clients always send commands as RESP arrays of bulk strings.
pub fn extract_command(val: &RespValue) -> Result<Command, String> {
    match val {
        RespValue::Array(items) if !items.is_empty() => {
            let name = match &items[0] {
                RespValue::BulkString(b) => {
                    String::from_utf8(b.clone())
                        .map(|s| s.to_uppercase())
                        .map_err(|_| "Non-UTF8 command name".to_string())?
                }
                _ => return Err("Command name must be a bulk string".to_string()),
            };

            let args: Result<Vec<Vec<u8>>, String> = items[1..]
                .iter()
                .map(|item| match item {
                    RespValue::BulkString(b) => Ok(b.clone()),
                    _ => Err("Command args must be bulk strings".to_string()),
                })
                .collect();

            Ok(Command { name, args: args? })
        }
        _ => Err("Expected RESP array for command".to_string()),
    }
}

// ── Common response builders ──────────────────────────────────────────────────

pub fn ok() -> RespValue {
    RespValue::SimpleString("OK".to_string())
}

pub fn pong() -> RespValue {
    RespValue::SimpleString("PONG".to_string())
}

pub fn err(msg: &str) -> RespValue {
    RespValue::Error(format!("ERR {}", msg))
}

pub fn wrong_args(cmd: &str) -> RespValue {
    RespValue::Error(format!(
        "ERR wrong number of arguments for '{}' command",
        cmd.to_lowercase()
    ))
}

pub fn nil() -> RespValue {
    RespValue::Nil
}

pub fn integer(n: i64) -> RespValue {
    RespValue::Integer(n)
}

pub fn bulk(data: Vec<u8>) -> RespValue {
    RespValue::BulkString(data)
}