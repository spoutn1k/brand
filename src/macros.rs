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
            w.session_storage()
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
            .ok_or(Error::NoWindow)?
            .document()
            .ok_or(Error::NoDocument)?
            .query_selector($selector)?
            .ok_or(Error::SelectorFailed($selector.to_string()))?
    }};

    ($selector:expr, $type:ty) => {{ query_selector!($selector).dyn_into::<$type>()? }};
}

macro_rules! roll_input {
    ($field:ident, $data:expr) => {{
        query_id!(
            &format!("roll-{}-input", stringify!($field)),
            web_sys::HtmlInputElement
        )
        .inspect(|e| e.set_value($data.$field.as_ref().unwrap_or(&String::new())))
    }};
}

macro_rules! roll_placeholder {
    ($field:ident, $placeholder:expr) => {{
        query_id!(
            &format!("roll-{}-input", stringify!($field)),
            web_sys::HtmlInputElement
        )
        .inspect(|e| {
            let _ = e.set_attribute("placeholder", $placeholder);
        })
    }};
}

macro_rules! el {
    ($tag:expr) => {
        web_sys::window()
            .ok_or(Error::NoWindow)?
            .document()
            .ok_or(Error::NoDocument)?
            .create_element($tag)?
    };

    ($tag:expr, $type:ty) => {
        el!($tag).unchecked_into::<$type>()
    };
}

macro_rules! event_target {
    ($event:expr) => {
        $event.target().ok_or(Error::NoTarget)?
    };

    ($event:expr, $type:ty) => {
        event_target!($event).unchecked_into::<$type>()
    };
}

pub(crate) use body;
pub(crate) use document;
pub(crate) use el;
pub(crate) use event_target;
pub(crate) use query_id;
pub(crate) use query_selector;
pub(crate) use roll_input;
pub(crate) use roll_placeholder;
pub(crate) use storage;
