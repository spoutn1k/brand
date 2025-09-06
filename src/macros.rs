use crate::Error;

macro_rules! document {
    () => {{
        web_sys::window()
            .ok_or(Error::NoWindow)
            .and_then(|w| w.document().ok_or(Error::NoDocument))
    }};
}

macro_rules! body {
    () => {{
        web_sys::window()
            .ok_or(Error::NoWindow)
            .and_then(|w| w.document().ok_or(Error::NoDocument))
            .and_then(|d| d.body().ok_or(Error::NoDocument))
    }};
}

macro_rules! storage {
    () => {{
        web_sys::window().ok_or(Error::NoWindow).and_then(|w| {
            w.local_storage()
                .map_err(|e| Error::Js(e.as_string().unwrap_or_default()))
                .and_then(|s| s.ok_or(Error::NoStorage))
        })
    }};
}

pub trait SessionStorageExt {
    fn get_existing(&self, key: &str) -> Result<String, Error>;
}

impl SessionStorageExt for web_sys::Storage {
    fn get_existing(&self, key: &str) -> Result<String, Error> {
        self.get_item(key)?
            .ok_or_else(|| Error::MissingKey(key.to_string()))
    }
}

macro_rules! query_id {
    ($id:expr, $type:ty) => {{ query_id!($id).map(|e| e.unchecked_into::<$type>()) }};

    ($id:expr) => {{
        web_sys::window()
            .ok_or(Error::NoWindow)
            .and_then(|w| w.document().ok_or(Error::NoDocument))
            .and_then(|d| {
                d.get_element_by_id($id)
                    .ok_or(Error::NoElementId($id.to_string()))
            })
    }};
}

macro_rules! query_selector {
    ($selector:expr) => {{
        web_sys::window()
            .ok_or(Error::NoWindow)
            .and_then(|w| w.document().ok_or(Error::NoDocument))
            .and_then(|d| {
                d.query_selector($selector)
                    .transpose()
                    .ok_or(Error::SelectorFailed($selector.to_string()))
                    .and_then(|r| r.map_err(Error::from))
            })
    }};

    ($selector:expr, $type:ty) => {{ query_selector!($selector).and_then(|e| e.dyn_into::<$type>()) }};
}

macro_rules! el {
    ($tag:expr) => {
        web_sys::window()
            .ok_or(Error::NoWindow)
            .and_then(|w| w.document().ok_or(Error::NoDocument))
            .and_then(|d| d.create_element($tag).map_err(Error::from))
    };

    ($tag:expr, $type:ty) => {
        el!($tag).map(|e| e.unchecked_into::<$type>())
    };
}

macro_rules! event_target {
    ($event:expr) => {
        $event.target().ok_or(Error::NoTarget)
    };

    ($event:expr, $type:ty) => {
        event_target!($event).map(|e| e.unchecked_into::<$type>())
    };
}

pub(crate) use body;
pub(crate) use document;
pub(crate) use el;
pub(crate) use event_target;
pub(crate) use query_id;
pub(crate) use query_selector;
pub(crate) use storage;
