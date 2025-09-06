pub mod editor {
    use crate::{
        Aquiesce, Error, QueryExt, fs, storage,
        view::{landing, preview},
    };
    use web_sys::HtmlInputElement;

    pub fn hide() -> Result<(), Error> {
        "editor".query_id()?.class_list().add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        "editor".query_id()?.class_list().remove_1("hidden")?;

        Ok(())
    }

    pub fn reset() -> Result<(), Error> {
        preview::reset()?;

        "photoselect"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");

        wasm_bindgen_futures::spawn_local(async move {
            fs::clear_dir("").await.aquiesce();
        });

        storage()?.clear()?;

        landing::show()?;
        hide()?;

        Ok(())
    }
}

pub mod landing {
    use crate::{Error, EventTargetExt, JsResult, QueryExt, error::Aquiesce, fs};
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, FileSystemFileEntry, HtmlInputElement, InputEvent};

    thread_local! {
    static DRAG_FILES: Closure<dyn Fn(InputEvent) -> JsResult> =
        Closure::new(move |event: InputEvent| -> JsResult {
            let files = event.target_into::<HtmlInputElement>()?
                .webkit_entries()
                .iter()
                .map(|f| f.unchecked_into::<FileSystemFileEntry>())
                .collect::<Vec<_>>();

            wasm_bindgen_futures::spawn_local(async move {
                crate::setup_editor_from_files(&files).await.aquiesce()
            });

            Ok(())
        });

    static CLEAR_STORAGE: Closure<dyn Fn(Event)> = Closure::new(|_| fs::clear());
    static INHIBIT: Closure<dyn Fn(Event)> = Closure::new(|e: Event| e.prevent_default());
    }

    pub async fn landing_stats() -> Result<(), Error> {
        let count = fs::file_count().await;

        "nerd-files"
            .query_id()?
            .set_text_content(Some(&format!("Filesystem contains {count} files")));

        let count = web_sys::window()
            .ok_or(Error::NoWindow)?
            .navigator()
            .hardware_concurrency();

        "nerd-threads"
            .query_id()?
            .set_text_content(Some(&format!("Browser can access {count} threads")));

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        DRAG_FILES
            .try_with(|h| {
                "photoselect"
                    .query_id_into::<HtmlInputElement>()?
                    .add_event_listener_with_callback("change", h.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        INHIBIT
            .try_with(|c| {
                "photoselect"
                    .query_id_into::<HtmlInputElement>()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        CLEAR_STORAGE
            .try_with(|handler| {
                "clear-storage"
                    .query_id()?
                    .add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }

    pub fn hide() -> Result<(), Error> {
        "landing".query_id()?.class_list().add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        "landing".query_id()?.class_list().remove_1("hidden")?;

        Ok(())
    }
}

pub mod preview {
    use crate::{
        Aquiesce, AsHtmlExt, Error, EventTargetExt, QueryExt, controller, controller::Update,
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlElement, MouseEvent};

    thread_local! {
    static CLICK_EXPOSURE: Closure<dyn Fn(MouseEvent)> = Closure::new(handle_exposure_click);

    static SELECTION_CLEAR: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::SelectionClear).aquiesce());

    static SELECTION_ALL: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::SelectionAll).aquiesce());

    static SELECTION_INVERT: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::SelectionInvert).aquiesce());
    }

    fn handle_exposure_click(event: MouseEvent) {
        fn inner(event: MouseEvent) -> Result<(), Error> {
            let shift = event.shift_key();
            let ctrl = event.ctrl_key();
            let meta = event.meta_key();

            let target = event.target_into::<HtmlElement>()?;

            let data = target
                .get_attribute("data-exposure-index")
                .ok_or(Error::MissingKey("img does not contain index".into()))?;
            let index = data.parse::<u32>()?;

            log::info!(
                "Clicked on exposure {index} with shift: {shift}, ctrl: {ctrl}, meta: {meta}"
            );

            controller::toggle_selection(index, shift, ctrl | meta)?;

            Ok(())
        }

        inner(event).aquiesce();
    }

    pub fn create(count: u32) -> Result<(), Error> {
        for index in 1..=count {
            let image = "img".as_html()?;
            image.set_id(&format!("exposure-{index}-preview"));
            image.set_attribute("alt", &format!("E{}", index))?;
            image.set_attribute("data-exposure-index", &index.to_string())?;

            CLICK_EXPOSURE
                .try_with(|h| {
                    image.add_event_listener_with_callback("click", h.as_ref().unchecked_ref())
                })
                .map_err(Error::from)??;

            "preview-thumbnails"
                .query_id()?
                .append_with_node_1(&image)?;
        }

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        SELECTION_CLEAR
            .try_with(|c| {
                "selection-clear"
                    .query_id()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        SELECTION_ALL
            .try_with(|c| {
                "selection-all"
                    .query_id()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        SELECTION_INVERT
            .try_with(|c| {
                "selection-invert"
                    .query_id()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }

    pub fn reset() -> Result<(), Error> {
        "preview-thumbnails".query_id()?.set_inner_html("");

        Ok(())
    }
}

pub mod exposure {
    use crate::{
        Aquiesce, Error, EventTargetExt, JsError, JsResult, QueryExt, bindings,
        controller::{self, UIExposureUpdate, Update},
        models::{self, HTML_INPUT_TIMESTAMP_FORMAT},
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlInputElement};

    thread_local! {
    static PROMPT_GPS: Closure<dyn Fn(Event)> = Closure::new(|_| bindings::prompt_coords());

    static ROTATE_LEFT: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::RotateLeft).aquiesce());

    static ROTATE_RIGHT: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::RotateRight).aquiesce());
    }

    fn set_handler(
        field: impl Fn(String) -> UIExposureUpdate + 'static + Clone,
        input: &web_sys::Element,
    ) -> JsResult {
        let handler = Closure::<dyn Fn(_) -> JsResult>::new(move |event: Event| -> JsResult {
            controller::update(Update::ExposureField(field(
                event.target_into::<HtmlInputElement>()?.value(),
            )))
            .js_error()
        });

        input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
        handler.forget();

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        let sspeed_input = "input#exposures-sspeed-input".query_selector()?;
        let aperture_input = "input#exposures-aperture-input".query_selector()?;
        let lens_input = "input#exposures-lens-input".query_selector()?;
        let comment_input = "input#exposures-comment-input".query_selector()?;
        let date_input = "input#exposures-date-input".query_selector()?;
        let gps_input = "input#exposures-gps-input".query_selector()?;

        set_handler(UIExposureUpdate::ShutterSpeed, &sspeed_input)?;
        set_handler(UIExposureUpdate::Aperture, &aperture_input)?;
        set_handler(UIExposureUpdate::Lens, &lens_input)?;
        set_handler(UIExposureUpdate::Comment, &comment_input)?;
        set_handler(UIExposureUpdate::Date, &date_input)?;
        set_handler(UIExposureUpdate::Gps, &gps_input)?;

        PROMPT_GPS
            .try_with(|c| {
                "button#exposures-gps-button"
                    .query_selector()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        ROTATE_LEFT
            .try_with(|c| {
                "rotate-left"
                    .query_id()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        ROTATE_RIGHT
            .try_with(|c| {
                "rotate-right"
                    .query_id()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }

    pub fn set_contents(
        title: String,
        contents: &models::ExposureSpecificData,
    ) -> Result<(), Error> {
        "div#exposures-title"
            .query_selector()?
            .set_text_content(Some(title.as_str()));

        "exposures-sspeed-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(contents.sspeed.as_deref().unwrap_or_default());
        "exposures-aperture-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(contents.aperture.as_deref().unwrap_or_default());
        "exposures-lens-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(contents.lens.as_deref().unwrap_or_default());
        "exposures-comment-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(contents.comment.as_deref().unwrap_or_default());
        "exposures-date-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(
                contents
                    .date
                    .map(|d| d.format(HTML_INPUT_TIMESTAMP_FORMAT).to_string())
                    .as_deref()
                    .unwrap_or_default(),
            );
        "exposures-gps-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(
                contents
                    .gps
                    .map(|(la, lo)| format!("{la}, {lo}"))
                    .as_deref()
                    .unwrap_or_default(),
            );

        Ok(())
    }

    pub fn hide() -> Result<(), Error> {
        "div#exposure-specific"
            .query_selector()?
            .class_list()
            .add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        "div#exposure-specific"
            .query_selector()?
            .class_list()
            .remove_1("hidden")?;

        Ok(())
    }
}

pub mod roll {
    use crate::{
        Aquiesce, Error, EventTargetExt, JsError, JsResult, QueryExt,
        controller::{self, UIRollUpdate, Update},
        models::RollData,
        view,
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlInputElement};

    thread_local! {
    static RESET_EDITOR: Closure<dyn Fn(Event)> =
        Closure::new(|_| view::editor::reset().aquiesce());

    static EXPORT: Closure<dyn Fn(Event)> = Closure::new(|_| {
        wasm_bindgen_futures::spawn_local(async {
            crate::process_images().await.aquiesce();
        });
    });
    }

    fn set_handler(
        field: impl Fn(String) -> UIRollUpdate + 'static + Clone,
        input: &web_sys::Element,
    ) -> JsResult {
        let handler = Closure::<dyn Fn(Event) -> JsResult>::new(move |event: Event| -> JsResult {
            controller::update(Update::Roll(field(
                event.target_into::<HtmlInputElement>()?.value(),
            )))
            .js_error()
        });

        input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
        handler.forget();

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        let author_input = "roll-author-input".query_id()?;
        let make_input = "roll-make-input".query_id()?;
        let model_input = "roll-model-input".query_id()?;
        let iso_input = "roll-iso-input".query_id()?;
        let description_input = "roll-description-input".query_id()?;

        set_handler(UIRollUpdate::Author, &author_input)?;
        set_handler(UIRollUpdate::Make, &make_input)?;
        set_handler(UIRollUpdate::Model, &model_input)?;
        set_handler(UIRollUpdate::Iso, &iso_input)?;
        set_handler(UIRollUpdate::Film, &description_input)?;

        RESET_EDITOR
            .try_with(|c| {
                "editor-reset"
                    .query_id()?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        EXPORT
            .try_with(|h| {
                "download"
                    .query_id()?
                    .add_event_listener_with_callback("click", h.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }

    pub fn fill_fields(data: &RollData) -> JsResult {
        "roll-author-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(data.author.as_deref().unwrap_or_default());

        "roll-make-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(data.make.as_deref().unwrap_or_default());

        "roll-model-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(data.model.as_deref().unwrap_or_default());

        "roll-iso-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(data.iso.as_deref().unwrap_or_default());

        "roll-description-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(data.description.as_deref().unwrap_or_default());

        Ok(())
    }

    pub fn hide() -> Result<(), Error> {
        "div#roll".query_selector()?.class_list().add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        "div#roll"
            .query_selector()?
            .class_list()
            .remove_1("hidden")?;

        Ok(())
    }
}
