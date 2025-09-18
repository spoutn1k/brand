mod map;

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
    use crate::{
        Error, EventTargetExt, JsResult, QueryExt, SetEventHandlerExt, error::Aquiesce, fs,
    };
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
        "photoselect"
            .query_id()?
            .on("change", &DRAG_FILES)?
            .on("click", &INHIBIT)?;

        "clear-storage".query_id()?.on("click", &CLEAR_STORAGE)?;

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
        Aquiesce, AsHtmlExt, Error, EventTargetExt, QueryExt, SetEventHandlerExt,
        controller::{self, Update},
        models,
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
            let target = event.target_into::<HtmlElement>()?;

            let data = target
                .get_attribute("data-exposure-index")
                .ok_or(Error::MissingKey("img does not contain index".into()))?;
            let index = data.parse::<u32>()?;

            controller::update(Update::SelectExposure(
                index,
                event.shift_key(),
                event.ctrl_key() | event.meta_key(),
            ))?;

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

            image.on("click", &CLICK_EXPOSURE)?;

            "preview-thumbnails"
                .query_id()?
                .append_with_node_1(&image)?;
        }

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        "selection-clear"
            .query_id()?
            .on("click", &SELECTION_CLEAR)?;

        "selection-all".query_id()?.on("click", &SELECTION_ALL)?;

        "selection-invert"
            .query_id()?
            .on("click", &SELECTION_INVERT)?;

        Ok(())
    }

    pub fn reset() -> Result<(), Error> {
        "preview-thumbnails".query_id()?.set_inner_html("");

        Ok(())
    }

    pub fn reflect_selection(
        all: &models::Selection,
        selection: &models::Selection,
    ) -> Result<(), Error> {
        for index in all.items() {
            let image = format!("exposure-{index}-preview").query_id()?;

            if selection.contains(index) {
                image.class_list().add_1("selected")?;
            } else {
                image.class_list().remove_1("selected")?;
            }
        }

        Ok(())
    }
}

pub mod exposure {
    use crate::{
        Aquiesce, Error, EventTargetExt, QueryExt, SetEventHandlerExt, bindings,
        controller::{self, UIExposureUpdate, Update},
        models::{self, HTML_INPUT_TIMESTAMP_FORMAT},
        view::map,
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlInputElement};

    thread_local! {
    static PROMPT_GPS: Closure<dyn Fn(Event)> = Closure::new(|_| bindings::prompt_coords());

    static ROTATE_LEFT: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::RotateLeft).aquiesce());

    static ROTATE_RIGHT: Closure<dyn Fn(Event)> =
        Closure::new(|_| controller::update(Update::RotateRight).aquiesce());

    static UPDATE_SSPEED: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Exposure(UIExposureUpdate::ShutterSpeed(t.value()))))
            .aquiesce()
    });

    static UPDATE_APERTURE: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Exposure(UIExposureUpdate::Aperture(t.value()))))
            .aquiesce()
    });

    static UPDATE_LENS: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Exposure(UIExposureUpdate::Lens(t.value()))))
            .aquiesce()
    });

    static UPDATE_COMMENT: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Exposure(UIExposureUpdate::Comment(t.value()))))
            .aquiesce()
    });

    static UPDATE_DATE: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Exposure(UIExposureUpdate::Date(t.value()))))
            .aquiesce()
    });

    static UPDATE_GPS: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Exposure(UIExposureUpdate::Gps(t.value()))))
            .aquiesce()
    });
    }

    pub fn setup() -> Result<(), Error> {
        "input#exposures-sspeed-input"
            .query_selector()?
            .on("input", &UPDATE_SSPEED)?;
        "input#exposures-aperture-input"
            .query_selector()?
            .on("input", &UPDATE_APERTURE)?;
        "input#exposures-lens-input"
            .query_selector()?
            .on("input", &UPDATE_LENS)?;
        "input#exposures-comment-input"
            .query_selector()?
            .on("input", &UPDATE_COMMENT)?;
        "input#exposures-date-input"
            .query_selector()?
            .on("input", &UPDATE_DATE)?;
        "input#exposures-gps-input"
            .query_selector()?
            .on("input", &UPDATE_GPS)?;

        "button#exposures-gps-button"
            .query_selector()?
            .on("click", &PROMPT_GPS)?;

        "rotate-left".query_id()?.on("click", &ROTATE_LEFT)?;
        "rotate-right".query_id()?.on("click", &ROTATE_RIGHT)?;

        map::setup().aquiesce();

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

        if let Some((lat, lon)) = contents.gps {
            map::show_location(lat, lon);
        }

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

        map::invalidate();

        Ok(())
    }
}

pub mod roll {
    use crate::{
        Aquiesce, Error, EventTargetExt, QueryExt, SetEventHandlerExt,
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

    static UPDATE_AUTHOR: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Roll(UIRollUpdate::Author(t.value()))))
            .aquiesce()
    });

    static UPDATE_MAKE: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Roll(UIRollUpdate::Make(t.value()))))
            .aquiesce()
    });

    static UPDATE_MODEL: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Roll(UIRollUpdate::Model(t.value()))))
            .aquiesce()
    });

    static UPDATE_ISO: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Roll(UIRollUpdate::Iso(t.value()))))
            .aquiesce()
    });

    static UPDATE_DESCRIPTION: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| controller::update(Update::Roll(UIRollUpdate::Film(t.value()))))
            .aquiesce()
    });
    }

    pub fn setup() -> Result<(), Error> {
        "roll-author-input"
            .query_id()?
            .on("input", &UPDATE_AUTHOR)?;
        "roll-make-input".query_id()?.on("input", &UPDATE_MAKE)?;
        "roll-model-input".query_id()?.on("input", &UPDATE_MODEL)?;
        "roll-iso-input".query_id()?.on("input", &UPDATE_ISO)?;
        "roll-description-input"
            .query_id()?
            .on("input", &UPDATE_DESCRIPTION)?;

        "editor-reset".query_id()?.on("click", &RESET_EDITOR)?;
        "download".query_id()?.on("click", &EXPORT)?;

        Ok(())
    }

    pub fn fill_fields(data: &RollData) -> Result<(), Error> {
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
