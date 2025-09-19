use std::thread::AccessError;
use wasm_bindgen::JsValue;

pub type JsResult<T = ()> = Result<T, JsValue>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("JS failure: {0}")]
    Js(String),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    SerdeWasm(#[from] serde_wasm_bindgen::Error),
    #[error("Missing key from storage: {0}")]
    MissingKey(String),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),
    #[error("Failed to parse TSE file: {0}")]
    ParseTse(String),
    #[error(transparent)]
    Logging(#[from] log::SetLoggerError),
    #[error(transparent)]
    Tiff(#[from] tiff::TiffError),
    #[error(transparent)]
    Unsupported(#[from] image::error::UnsupportedError),
    #[error(transparent)]
    Format(#[from] std::fmt::Error),
    #[error(transparent)]
    ThreadVariable(#[from] AccessError),
    #[error("Failed to access global window")]
    NoWindow,
    #[error("Failed to access document on global window")]
    NoDocument,
    #[error("Failed to access document body")]
    NoBody,
    #[error("Failed to access `session_storage` on global `window`")]
    NoStorage,
    #[error("Failed to access element with id {0}")]
    NoElementId(String),
    #[error("Query returned no results: {0}")]
    SelectorFailed(String),
    #[error("No target for event !")]
    NoTarget,
    #[error(transparent)]
    ChronoParse(#[from] chrono::ParseError),
    #[error("Bad GPS element format:\n{0}")]
    GpsParse(String),
    #[error("Map already initialized")]
    MapInit,
    #[error(transparent)]
    MpscChannelSend(#[from] futures::channel::mpsc::SendError),
    #[error("Failed to send through the channel")]
    OsChannelSend,
    #[error(transparent)]
    OsChannelRecv(#[from] futures::channel::oneshot::Canceled),
}

impl From<JsValue> for Error {
    fn from(value: JsValue) -> Self {
        Error::Js(
            value
                .as_string()
                .unwrap_or_else(|| "Unknown JS error".to_string()),
        )
    }
}

impl From<Error> for JsValue {
    fn from(value: Error) -> Self {
        JsValue::from_str(&value.to_string())
    }
}

pub trait JsError<T> {
    fn js_error(self) -> JsResult<T>;
}

impl<T, E: std::error::Error> JsError<T> for Result<T, E> {
    fn js_error(self) -> JsResult<T> {
        self.map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

pub trait Aquiesce {
    fn aquiesce(self);
}

impl<E: std::error::Error> Aquiesce for Result<(), E> {
    fn aquiesce(self) {
        if let Err(e) = self {
            log::error!("Error: {}", e);
        }
    }
}
