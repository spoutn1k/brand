pub mod landing {
    use crate::{
        Error, JsResult,
        error::Aquiesce,
        fs,
        macros::{event_target, query_id},
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, FileSystemFileEntry, HtmlInputElement, InputEvent};

    thread_local! {
    static DRAG_FILES: Closure<dyn Fn(InputEvent) -> JsResult> =
        Closure::new(move |event: InputEvent| -> JsResult {
            let files = event_target!(event, HtmlInputElement)?
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

        query_id!("nerd-files")?
            .set_text_content(Some(&format!("Filesystem contains {count} files")));

        let count = web_sys::window()
            .ok_or(Error::NoWindow)?
            .navigator()
            .hardware_concurrency();

        query_id!("nerd-threads")?
            .set_text_content(Some(&format!("Browser can access {count} threads")));

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        DRAG_FILES
            .try_with(|h| {
                query_id!("photoselect", HtmlInputElement)?
                    .add_event_listener_with_callback("change", h.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        INHIBIT
            .try_with(|c| {
                query_id!("photoselect", HtmlInputElement)?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        CLEAR_STORAGE
            .try_with(|handler| {
                query_id!("clear-storage")?
                    .add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }
}

pub mod preview {
    use crate::{
        Aquiesce, Error, controller,
        controller::Update,
        macros::{el, event_target, query_id},
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
            let target = event_target!(event, HtmlElement)?;

            let data = target
                .get_attribute("data-exposure-index")
                .ok_or(Error::MissingKey("img does not contain index".into()))?;
            let index = data.parse::<u32>()?;

            let shift = event.shift_key();
            let ctrl = event.ctrl_key();
            let meta = event.meta_key();

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
            let image = el!("img")?;
            image.set_id(&format!("exposure-{index}-preview"));
            image.set_attribute("alt", &format!("E{}", index))?;
            image.set_attribute("data-exposure-index", &index.to_string())?;
            CLICK_EXPOSURE
                .try_with(|h| {
                    image.add_event_listener_with_callback("click", h.as_ref().unchecked_ref())
                })
                .map_err(Error::from)??;
            query_id!("preview")?.append_with_node_1(&image)?;
        }

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        SELECTION_CLEAR
            .try_with(|c| {
                query_id!("selection-clear")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        SELECTION_ALL
            .try_with(|c| {
                query_id!("selection-all")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        SELECTION_INVERT
            .try_with(|c| {
                query_id!("selection-invert")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }
}

pub mod exposure {
    use crate::{
        Aquiesce, Error, JsError, JsResult, bindings,
        controller::{self, UIExposureUpdate, Update},
        macros::{event_target, query_id, query_selector},
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
            log::info!("clicked");
            controller::update(Update::ExposureField(field(
                event_target!(event, HtmlInputElement)?.value(),
            )))
            .js_error()
        });

        input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
        handler.forget();

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        let sspeed_input = query_selector!("input#exposures-sspeed-input")?;
        let aperture_input = query_selector!("input#exposures-aperture-input")?;
        let lens_input = query_selector!("input#exposures-lens-input")?;
        let comment_input = query_selector!("input#exposures-comment-input")?;
        let date_input = query_selector!("input#exposures-date-input")?;
        let gps_input = query_selector!("input#exposures-gps-input")?;

        set_handler(UIExposureUpdate::ShutterSpeed, &sspeed_input)?;
        set_handler(UIExposureUpdate::Aperture, &aperture_input)?;
        set_handler(UIExposureUpdate::Lens, &lens_input)?;
        set_handler(UIExposureUpdate::Comment, &comment_input)?;
        set_handler(UIExposureUpdate::Date, &date_input)?;
        set_handler(UIExposureUpdate::Gps, &gps_input)?;

        PROMPT_GPS
            .try_with(|c| {
                query_selector!("button#exposures-gps-button")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        ROTATE_LEFT
            .try_with(|c| {
                query_id!("rotate-left")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        ROTATE_RIGHT
            .try_with(|c| {
                query_id!("rotate-right")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }

    pub fn set_contents(
        title: String,
        contents: &models::ExposureSpecificData,
    ) -> Result<(), Error> {
        query_selector!("div#exposures-title")?.set_text_content(Some(title.as_str()));

        query_id!("exposures-sspeed-input", HtmlInputElement)?
            .set_value(contents.sspeed.as_deref().unwrap_or_default());
        query_id!("exposures-aperture-input", HtmlInputElement)?
            .set_value(contents.aperture.as_deref().unwrap_or_default());
        query_id!("exposures-lens-input", HtmlInputElement)?
            .set_value(contents.lens.as_deref().unwrap_or_default());
        query_id!("exposures-comment-input", HtmlInputElement)?
            .set_value(contents.comment.as_deref().unwrap_or_default());
        query_id!("exposures-date-input", HtmlInputElement)?.set_value(
            contents
                .date
                .map(|d| d.format(HTML_INPUT_TIMESTAMP_FORMAT).to_string())
                .as_deref()
                .unwrap_or_default(),
        );
        query_id!("exposures-gps-input", HtmlInputElement)?.set_value(
            contents
                .gps
                .map(|(la, lo)| format!("{la}, {lo}"))
                .as_deref()
                .unwrap_or_default(),
        );

        Ok(())
    }

    pub fn hide() -> Result<(), Error> {
        query_selector!("div#exposure-specific")?
            .class_list()
            .add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        query_selector!("div#exposure-specific")?
            .class_list()
            .remove_1("hidden")?;

        Ok(())
    }
}

pub mod roll {
    use crate::{
        Aquiesce, Error, JsError, JsResult,
        controller::{self, UIRollUpdate, Update},
        macros::{event_target, query_id, query_selector},
        models::RollData,
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlInputElement};

    thread_local! {
    static RESET_EDITOR: Closure<dyn Fn(Event) -> JsResult> =
        Closure::new(|_| crate::reset_editor());

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
                event_target!(event, HtmlInputElement)?.value(),
            )))
            .js_error()
        });

        input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
        handler.forget();

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        let author_input = query_id!("roll-author-input")?;
        let make_input = query_id!("roll-make-input")?;
        let model_input = query_id!("roll-model-input")?;
        let iso_input = query_id!("roll-iso-input")?;
        let description_input = query_id!("roll-description-input")?;

        set_handler(UIRollUpdate::Author, &author_input)?;
        set_handler(UIRollUpdate::Make, &make_input)?;
        set_handler(UIRollUpdate::Model, &model_input)?;
        set_handler(UIRollUpdate::Iso, &iso_input)?;
        set_handler(UIRollUpdate::Film, &description_input)?;

        RESET_EDITOR
            .try_with(|c| {
                query_id!("editor-reset")?
                    .add_event_listener_with_callback("click", c.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        EXPORT
            .try_with(|h| {
                query_id!("download")?
                    .add_event_listener_with_callback("click", h.as_ref().unchecked_ref())
            })
            .map_err(Error::from)??;

        Ok(())
    }

    pub fn fill_fields(data: &RollData) -> JsResult {
        query_id!("roll-author-input", web_sys::HtmlInputElement)?
            .set_value(data.author.as_deref().unwrap_or_default());

        query_id!("roll-make-input", web_sys::HtmlInputElement)?
            .set_value(data.make.as_deref().unwrap_or_default());

        query_id!("roll-model-input", web_sys::HtmlInputElement)?
            .set_value(data.model.as_deref().unwrap_or_default());

        query_id!("roll-iso-input", web_sys::HtmlInputElement)?
            .set_value(data.iso.as_deref().unwrap_or_default());

        query_id!("roll-description-input", web_sys::HtmlInputElement)?
            .set_value(data.description.as_deref().unwrap_or_default());

        Ok(())
    }

    pub fn hide() -> Result<(), Error> {
        query_selector!("div#roll")?.class_list().add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        query_selector!("div#roll")?
            .class_list()
            .remove_1("hidden")?;

        Ok(())
    }
}
