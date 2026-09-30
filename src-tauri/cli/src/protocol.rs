use std::io::{Read, Write};

use openrize_core::readonly::{EntryPage, Status};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_REQUEST: usize = 8192;
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    pub operation: Operation,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Operation {
    Status {},
    Entries {
        from: u64,
        to: u64,
        status: Option<String>,
        limit: u32,
        full: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Data {
    Status(Status),
    Entries(EntryPage),
    Installation { executable: String, path: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub schema_version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Data>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    pub help: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl Response {
    pub fn success(data: Data) -> Self {
        Self {
            schema_version: VERSION,
            ok: true,
            data: Some(data),
            error: None,
            help: Vec::new(),
        }
    }

    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            schema_version: VERSION,
            ok: false,
            data: None,
            error: Some(ApiError {
                code: code.into(),
                message: message.into(),
            }),
            help: vec!["openrize --help".into()],
        }
    }
}

pub fn write_frame<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
    max: usize,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("IPC payload exceeds limit".into());
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(&bytes).map_err(|e| e.to_string())
}

pub fn read_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut impl Read,
    max: usize,
) -> Result<T, String> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > max {
        return Err("IPC payload exceeds limit".into());
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
