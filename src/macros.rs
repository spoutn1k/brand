use wasm_bindgen::JsValue;

#[derive(Debug, thiserror::Error)]
pub enum MacroError {
    #[error("Failed to access global `window`")]
    NoWindow,
    #[error("Failed to access `document` on global `window`")]
    NoDocument,
    #[error("Failed to access `session_storage` on global `window`")]
    NoStorage,
    #[error("Failed to access element with id {0}")]
    NoElementId(String),
    #[error("Query returned no results: {0}")]
    SelectorFailed(String),
    #[error("No target for event !")]
    NoTarget,
}

impl From<MacroError> for JsValue {
    fn from(err: MacroError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

macro_rules! body {
    () => {{
        web_sys::window()
            .ok_or(MacroError::NoWindow)?
            .document()
            .ok_or(MacroError::NoDocument)?
            .body()
            .ok_or(MacroError::NoDocument)?
    }};
}

macro_rules! storage {
    () => {{
        web_sys::window()
            .ok_or(MacroError::NoWindow)?
            .session_storage()?
            .ok_or(MacroError::NoStorage)?
    }};
}

pub trait SessionStorageExt {
    fn get_existing(&self, key: &str) -> Result<String, crate::Error>;
}

impl SessionStorageExt for web_sys::Storage {
    fn get_existing(&self, key: &str) -> Result<String, crate::Error> {
        self.get_item(key)?
            .ok_or_else(|| crate::Error::MissingKey(key.to_string()))
    }
}

macro_rules! query_id {
    ($id:expr, $type:ty) => {{ query_id!($id).unchecked_into::<$type>() }};

    ($id:expr) => {{
        web_sys::window()
            .ok_or(MacroError::NoWindow)?
            .document()
            .ok_or(MacroError::NoDocument)?
            .get_element_by_id($id)
            .ok_or(MacroError::NoElementId($id.to_string()))?
    }};
}

macro_rules! query_selector {
    ($selector:expr) => {{
        web_sys::window()
            .ok_or(MacroError::NoWindow)?
            .document()
            .ok_or(MacroError::NoDocument)?
            .query_selector($selector)?
            .ok_or(MacroError::SelectorFailed($selector.to_string()))?
    }};

    ($selector:expr, $type:ty) => {{ query_selector!($selector).dyn_into::<$type>()? }};
}

macro_rules! roll_input {
    ($field:ident, $data:expr) => {{
        let tmp = query_id!(
            &format!("roll-{}-input", stringify!($field)),
            web_sys::HtmlInputElement
        );

        tmp.set_value($data.$field.as_ref().unwrap_or(&String::new()));

        tmp
    }};
}

macro_rules! roll_placeholder {
    ($field:ident, $placeholder:expr) => {{
        let tmp = query_id!(
            &format!("roll-{}-input", stringify!($field)),
            web_sys::HtmlInputElement
        );

        tmp.set_attribute("placeholder", $placeholder)?;

        tmp
    }};
}

macro_rules! el {
    ($tag:expr) => {
        web_sys::window()
            .ok_or(MacroError::NoWindow)?
            .document()
            .ok_or(MacroError::NoDocument)?
            .create_element($tag)?
    };

    ($tag:expr, $type:ty) => {
        el!($tag).unchecked_into::<$type>()
    };
}

macro_rules! event_target {
    ($event:expr) => {
        $event.target().ok_or(MacroError::NoTarget)?
    };

    ($event:expr, $type:ty) => {
        event_target!($event).unchecked_into::<$type>()
    };
}

pub(crate) use body;
pub(crate) use el;
pub(crate) use event_target;
pub(crate) use query_id;
pub(crate) use query_selector;
pub(crate) use roll_input;
pub(crate) use roll_placeholder;
pub(crate) use storage;
