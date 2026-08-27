//! Versioned, bounded length-prefixed JSON RPC framing.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use interprocess::local_socket::{prelude::*, GenericFilePath, GenericNamespaced, Listener, ListenerOptions, Stream};

pub const RPC_VERSION: &str = "1.0";
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRequest {
    pub version: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RpcResponse {
    pub version: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

impl RpcResponse {
    pub fn success(result: impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self { version: RPC_VERSION.into(), ok: true, result: Some(serde_json::to_value(result)?), error: None })
    }
    pub fn failure(error: impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self { version: RPC_VERSION.into(), ok: false, result: None, error: Some(serde_json::to_value(error)?) })
    }
}

pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("RPC frame exceeds {MAX_FRAME_BYTES} bytes")));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("RPC frame exceeds {MAX_FRAME_BYTES} bytes")));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub fn read_json<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> io::Result<T> {
    serde_json::from_slice(&read_frame(reader)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn endpoint() -> io::Result<interprocess::local_socket::Name<'static>> {
    if GenericNamespaced::is_supported() {
        "popu-ipc-host-v1".to_ns_name::<GenericNamespaced>()
    } else {
        "/tmp/popu-ipc-host-v1.sock".to_fs_name::<GenericFilePath>()
    }
}

pub fn connect() -> io::Result<Stream> { Stream::connect(endpoint()?) }

pub fn bind() -> io::Result<Listener> {
    ListenerOptions::new().name(endpoint()?).create_sync()
}

pub fn call(request: &RpcRequest) -> io::Result<RpcResponse> {
    let mut stream = connect()?;
    write_frame(&mut stream, request)?;
    read_json(&mut stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_frame_boundaries() {
        let mut bytes = vec![]; write_frame(&mut bytes, &serde_json::json!({"a": 1})).unwrap();
        write_frame(&mut bytes, &serde_json::json!({"b": 2})).unwrap();
        let mut input = bytes.as_slice();
        assert_eq!(read_json::<_, Value>(&mut input).unwrap()["a"], 1);
        assert_eq!(read_json::<_, Value>(&mut input).unwrap()["b"], 2);
    }
    #[test]
    fn rejects_oversized_frames_before_allocating() {
        let prefix = ((MAX_FRAME_BYTES as u32) + 1).to_be_bytes();
        let mut input = prefix.as_slice();
        assert_eq!(read_frame(&mut input).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
    #[test]
    fn rejects_malformed_json() {
        let mut input = vec![]; input.extend_from_slice(&1_u32.to_be_bytes()); input.push(b'{');
        assert_eq!(read_json::<_, Value>(&mut input.as_slice()).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
