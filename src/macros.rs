macro_rules! storage {
    () => {{
        web_sys::window()
            .ok_or("no global `window` exists")?
            .session_storage()?
            .ok_or("No storage for session !")?
    }};
}

macro_rules! query_id {
    ($id:expr, $type:ty) => {{ query_id!($id).dyn_into::<$type>()? }};

    ($id:expr) => {{
        web_sys::window()
            .ok_or("No window")?
            .document()
            .ok_or("no document on window")?
            .get_element_by_id($id)
            .ok_or(&format!("Failed to access element of id {}", $id))?
    }};
}

macro_rules! query_selector {
    ($selector:expr) => {{
        web_sys::window()
            .ok_or("No window")?
            .document()
            .ok_or("no document on window")?
            .query_selector($selector)?
            .ok_or(&format!("Failed to access element with {}", $selector))?
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
            .ok_or("No window")?
            .document()
            .ok_or("no document on window")?
            .create_element($tag)?
    };

    ($tag:expr, $type:ty) => {
        el!($tag).dyn_into::<$type>()?
    };
}

macro_rules! event_target {
    ($event:expr) => {
        $event.target().ok_or("No target for event !")?
    };

    ($event:expr, $type:ty) => {
        event_target!($event).dyn_into::<$type>()?
    };
}
