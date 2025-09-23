use crate::{Error, error::IntoError};
use js_sys::{Array, Uint8Array};
use std::thread::LocalKey;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Blob, Document, Element, Event, EventTarget, HtmlElement, Storage, Window};

pub fn window() -> Result<Window, Error> {
    web_sys::window().ok_or(Error::NoWindow)
}

pub fn document() -> Result<Document, Error> {
    window()?.document().ok_or(Error::NoDocument)
}

pub fn body() -> Result<HtmlElement, Error> {
    document()?.body().ok_or(Error::NoBody)
}

pub fn storage() -> Result<Storage, Error> {
    window()?.local_storage()?.ok_or(Error::NoStorage)
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

pub trait QueryExt {
    fn query_id_into<T: JsCast>(&self) -> Result<T, Error>;

    fn query_id(&self) -> Result<Element, Error> {
        Self::query_id_into(self)
    }

    fn query_selector_into<T: JsCast>(&self) -> Result<T, Error>;

    fn query_selector(&self) -> Result<Element, Error> {
        Self::query_selector_into(self)
    }

    fn query_selector_all_into<T: JsCast>(&self) -> Result<Vec<T>, Error>;

    fn query_selector_all(&self) -> Result<Vec<Element>, Error> {
        Self::query_selector_all_into(self)
    }
}

impl<S> QueryExt for S
where
    S: AsRef<str>,
{
    fn query_id_into<T: JsCast>(&self) -> Result<T, Error> {
        let element = document()?
            .get_element_by_id(self.as_ref())
            .ok_or(Error::NoElementId(self.as_ref().to_string()))?
            .unchecked_into::<T>();

        Ok(element)
    }

    fn query_selector_into<T: JsCast>(&self) -> Result<T, Error> {
        let element = document()?
            .query_selector(self.as_ref())?
            .ok_or(Error::SelectorFailed(self.as_ref().to_string()))?
            .unchecked_into::<T>();

        Ok(element)
    }

    fn query_selector_all_into<T: JsCast>(&self) -> Result<Vec<T>, Error> {
        let element = document()?
            .query_selector_all(self.as_ref())?
            .values()
            .into_iter()
            .collect::<Result<Vec<JsValue>, JsValue>>()?
            .into_iter()
            .map(|e| e.unchecked_into::<T>())
            .collect::<Vec<_>>();

        Ok(element)
    }
}

pub trait EventTargetExt {
    fn target(&self) -> Result<EventTarget, Error>;

    fn target_into<T: JsCast>(&self) -> Result<T, Error>;
}

impl<E> EventTargetExt for E
where
    E: Into<Event> + Clone,
{
    fn target(&self) -> Result<EventTarget, Error> {
        self.target_into()
    }

    fn target_into<T: JsCast>(&self) -> Result<T, Error> {
        let element = (*self)
            .clone()
            .into()
            .target()
            .ok_or(Error::NoTarget)?
            .unchecked_into::<T>();

        Ok(element)
    }
}

pub trait AsHtmlExt {
    fn as_html(&self) -> Result<Element, Error> {
        self.as_html_into()
    }

    fn as_html_into<T: JsCast>(&self) -> Result<T, Error>;
}

impl<S> AsHtmlExt for S
where
    S: AsRef<str>,
{
    fn as_html_into<T: JsCast>(&self) -> Result<T, Error> {
        let element = document()?.create_element(self.as_ref())?.unchecked_into();

        Ok(element)
    }
}

pub trait SetEventHandlerExt<C> {
    fn on(&self, event_type: &str, handler: &'static C) -> Result<&Self, Error>;
}

impl<E, C> SetEventHandlerExt<LocalKey<C>> for E
where
    E: Into<EventTarget> + Clone,
    C: AsRef<JsValue>,
{
    fn on(&self, event_type: &str, handler: &'static LocalKey<C>) -> Result<&Self, Error> {
        handler
            .try_with(|h| {
                self.clone()
                    .into()
                    .add_event_listener_with_callback(event_type, h.as_ref().unchecked_ref())
            })
            .error()??;

        Ok(self)
    }
}

pub fn download_buffer(buffer: &[u8], filename: &str, mime_type: &str) -> Result<(), Error> {
    let bytes = Uint8Array::new(&unsafe { Uint8Array::view(buffer) }.into());

    let array = Array::new();
    array.push(&bytes.buffer());

    let props = web_sys::BlobPropertyBag::new();
    props.set_type(mime_type);

    let blob = Blob::new_with_u8_array_sequence_and_options(&array, &props)?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let element = "a".as_html_into::<HtmlElement>()?;
    element.set_attribute("href", &url)?;
    element.set_attribute("download", filename)?;
    element.style().set_property("display", "none")?;

    body()?.append_with_node_1(&element)?;
    element.click();
    body()?.remove_child(&element)?;

    web_sys::Url::revoke_object_url(&url)?;

    Ok(())
}
