use crate::{controller, controller::Update, error::Aquiesce};
use wasm_bindgen::closure::Closure;
use web_sys::Event;

pub mod map;

fn update(kind: Update) -> Closure<dyn Fn(Event)> {
    Closure::new(move |_| controller::update(kind.clone()).aquiesce())
}

pub mod editor {
    use crate::{
        Aquiesce, Error, QueryExt, controller, fs,
        view::{landing, preview},
    };

    pub fn hide() -> Result<(), Error> {
        "editor".query_id()?.class_list().add_1("hidden")?;

        Ok(())
    }

    pub fn show() -> Result<(), Error> {
        "editor".query_id()?.class_list().remove_1("hidden")?;

        Ok(())
    }

    pub fn reset() -> Result<(), Error> {
        preview::reset().and(landing::reset_file_input())?;

        wasm_bindgen_futures::spawn_local(async move {
            fs::clear_dir("").await.aquiesce();
        });

        controller::clear_local_storage()?;

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

    pub fn reset_file_input() -> Result<(), Error> {
        "photoselect"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");

        Ok(())
    }

    pub fn setup() -> Result<(), Error> {
        reset_file_input()?;

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
        error::IntoError,
        image_management::RotateExt,
        models::{self, FileMetadata, Orientation},
        view::update,
    };
    use base64::{Engine, prelude::BASE64_STANDARD};
    use image::{ImageReader, codecs::jpeg::JpegEncoder};
    use std::{cell::RefCell, io::Cursor};
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlElement, HtmlImageElement, MouseEvent};

    thread_local! {
    static CLICK_EXPOSURE: Closure<dyn Fn(MouseEvent)> = Closure::new(handle_exposure_click);

    static SELECTION_CLEAR: Closure<dyn Fn(Event)> = update(Update::SelectionClear);
    static SELECTION_ALL: Closure<dyn Fn(Event)> = update(Update::SelectionAll);
    static SELECTION_INVERT: Closure<dyn Fn(Event)> = update(Update::SelectionInvert);
    static DRAGSTART: Closure<dyn Fn(Event)> =
        Closure::new(|e: Event| handle_dragstart(e).aquiesce());
    static DRAGOVER: Closure<dyn Fn(Event)> = Closure::new(|e: Event| {
        e.prevent_default();
        log::info!(
            "Dragged over `{}`",
            e.target_into::<HtmlElement>()
                .and_then(|t| t
                    .get_attribute("data-exposure-index")
                    .ok_or(Error::MissingKey("index".into())))
                .unwrap_or_default()
        )
    });
    static DROP: Closure<dyn Fn(Event)> =
        Closure::new(|e: Event| handle_drop(e).aquiesce());

    static DRAGGED: RefCell<u32> = const { RefCell::new(u32::MAX) };
    }

    fn handle_dragstart(event: Event) -> Result<(), Error> {
        let drag_target = event
            .target_into::<HtmlElement>()
            .and_then(|t| {
                t.get_attribute("data-exposure-index")
                    .ok_or(Error::MissingKey("index".into()))
            })
            .and_then(|i| i.parse::<u32>().error())?;

        log::info!("Started dragging `{drag_target}`");
        DRAGGED.set(drag_target);

        Ok(())
    }

    fn handle_drop(event: Event) -> Result<(), Error> {
        let drop_target = event
            .target_into::<HtmlElement>()
            .and_then(|t| {
                t.get_attribute("data-exposure-index")
                    .ok_or(Error::MissingKey("index".into()))
            })
            .and_then(|i| i.parse::<u32>().error())?;

        let drag_target = DRAGGED.take();
        log::info!("Dropped `{drag_target}` on `{drop_target}`");

        controller::update(Update::Reorder(drag_target, drop_target)).and_then(|_| reorder())?;

        Ok(())
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

    pub fn create(metadata: &[FileMetadata]) -> Result<(), Error> {
        let mut sorted = Vec::from_iter(metadata.iter());
        sorted.sort_by(|a, b| a.index.cmp(&b.index));

        let images = sorted
            .into_iter()
            .map(|file| -> Result<_, Error> {
                let image = "img".as_html()?;
                image.set_class_name("exposure-preview");
                image.set_attribute("alt", &format!("E{}", file.index))?;
                image.set_attribute("data-exposure-id", &file.name)?;
                image.set_attribute("data-exposure-index", &file.index.to_string())?;

                image.on("click", &CLICK_EXPOSURE)?;

                Ok(image)
            })
            .collect::<Result<Vec<_>, _>>()?;

        "preview-thumbnails"
            .query_id()?
            .replace_children_with_node(&js_sys::Array::from_iter(images));

        reorder()
    }

    pub fn reorder() -> Result<(), Error> {
        let mut images: Vec<_> = ".exposure-preview"
            .query_selector_all()?
            .into_iter()
            .filter_map(|image| {
                image
                    .get_attribute("data-exposure-index")
                    .and_then(|i| i.parse::<u32>().ok())
                    .map(|index| (index, image))
            })
            .collect();

        images.sort_by(|a, b| a.0.cmp(&b.0));

        "preview-thumbnails"
            .query_id()?
            .replace_children_with_node(&js_sys::Array::from_iter(
                images.into_iter().map(|(_, i)| i),
            ));

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
            let image =
                format!(".exposure-preview[data-exposure-index='{index}']").query_selector()?;

            if selection.contains(index) {
                image.class_list().add_1("selected")?;
            } else {
                image.class_list().remove_1("selected")?;
            }
        }

        Ok(())
    }

    pub fn rotate_thumbnail(index: u32, rotation: Orientation) -> Result<(), Error> {
        let image = format!(".exposure-preview[data-exposure-index='{index}']").query_selector()?;

        rotate_img(&image, rotation)
    }

    pub fn rotate_img(image: &web_sys::Element, rotation: Orientation) -> Result<(), Error> {
        if let Some(base64) = image
            .get_attribute("src")
            .and_then(|attr| attr.split_whitespace().nth(1).map(|b| b.to_string()))
        {
            let data = BASE64_STANDARD.decode(base64)?;
            let photo = ImageReader::new(Cursor::new(data))
                .with_guessed_format()?
                .decode()?
                .rotate(rotation);

            let mut thumbnail = vec![];
            JpegEncoder::new(&mut thumbnail).encode_image(&photo)?;

            let base64 = BASE64_STANDARD.encode(thumbnail);

            image.set_attribute("src", &format!("data:image/jpeg;base64, {base64}"))?;
        }

        Ok(())
    }

    fn rotate_contact_element(index: u32, rotation: Orientation) -> Result<(), Error> {
        let image =
            format!("#contact-sheet img[data-exposure-index='{index}']").query_selector()?;

        rotate_img(&image, rotation)
    }

    pub fn contact_sheet() -> Result<(), Error> {
        let preview = "preview-thumbnails"
            .query_id()?
            .clone_node_with_deep(true)?
            .unchecked_into::<HtmlElement>();

        js_sys::Array::from(&preview.children())
            .into_iter()
            .for_each(|node| {
                node.unchecked_into::<HtmlElement>()
                    .on("dragstart", &DRAGSTART)
                    .and_then(|node| node.on("dragover", &DRAGOVER))
                    .and_then(|node| node.on("drop", &DROP))
                    .and_then(|node| node.set_attribute("dragable", "true").error())
                    .aquiesce();
            });

        preview.set_id("contact-sheet");

        "editor".query_id()?.append_child(&preview)?;

        "#preview-thumbnails img[data-portrait]"
            .query_selector_all_into::<HtmlImageElement>()?
            .into_iter()
            .for_each(|img| {
                if let Some(index) = img
                    .get_attribute("data-exposure-index")
                    .and_then(|i| i.parse::<u32>().ok())
                {
                    rotate_contact_element(index, Orientation::Rotated90).aquiesce();
                }
            });

        Ok(())
    }

    pub fn contact_sheet_close() -> Result<(), Error> {
        "#editor>#contact-sheet"
            .query_selector()
            .map(|e| e.remove())
    }

    pub fn contact_sheet_open() -> Result<bool, Error> {
        "#editor>#contact-sheet".query_selector().map(|_| true)
    }
}

pub mod exposure {
    use crate::{
        Aquiesce, Error, EventTargetExt, QueryExt, SetEventHandlerExt,
        controller::{self, UIExposureUpdate, Update},
        gps::parse_gps,
        models::{self, HTML_INPUT_TIMESTAMP_FORMAT, Selection},
        view::{map, update},
    };
    use itertools::Itertools;
    use std::collections::BTreeSet;
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlInputElement};

    fn exposure(
        format: impl Fn(String) -> UIExposureUpdate + 'static + Clone,
    ) -> Closure<dyn Fn(Event)> {
        Closure::new(move |event: Event| {
            event
                .target_into::<HtmlInputElement>()
                .and_then(|t| controller::update(Update::Exposure(format.clone()(t.value()))))
                .aquiesce()
        })
    }

    thread_local! {
    static ROTATE_LEFT: Closure<dyn Fn(Event)> = update(Update::RotateLeft);
    static ROTATE_RIGHT: Closure<dyn Fn(Event)> = update(Update::RotateRight);

    static UPDATE_APERTURE: Closure<dyn Fn(Event)> = exposure(UIExposureUpdate::Aperture);
    static UPDATE_COMMENT: Closure<dyn Fn(Event)> = exposure(UIExposureUpdate::Comment);
    static UPDATE_DATE: Closure<dyn Fn(Event)> = exposure(UIExposureUpdate::Date);
    static UPDATE_LENS: Closure<dyn Fn(Event)> = exposure(UIExposureUpdate::Lens);
    static UPDATE_SSPEED: Closure<dyn Fn(Event)> = exposure(UIExposureUpdate::ShutterSpeed);
    static UPDATE_GPS: Closure<dyn Fn(Event)> = Closure::new(|event: Event| {
        event
            .target_into::<HtmlInputElement>()
            .and_then(|t| parse_gps(t.value()))
            .and_then(|(lat, lng)| {
                controller::update(Update::Exposure(UIExposureUpdate::Gps(lat, lng)))
            })
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

        "rotate-left".query_id()?.on("click", &ROTATE_LEFT)?;
        "rotate-right".query_id()?.on("click", &ROTATE_RIGHT)?;

        map::setup().aquiesce();

        Ok(())
    }

    pub fn one(index: u32, contents: &models::ExposureSpecificData) -> Result<(), Error> {
        "div#exposures-title"
            .query_selector()?
            .set_text_content(Some(&format!("Exposure {index}")));

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
            map::show_location(&[(lat, lon)]);
        } else {
            map::reset();
        }

        Ok(())
    }

    pub fn multiple(
        selection: &Selection,
        contents: &[models::ExposureSpecificData],
    ) -> Result<(), Error> {
        "div#exposures-title"
            .query_selector()?
            .set_text_content(Some(&format!("Exposures {selection}")));

        "exposures-sspeed-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");
        let sspeeds = contents
            .iter()
            .filter_map(|e| e.sspeed.as_deref())
            .collect::<BTreeSet<_>>();
        "exposures-sspeed-input"
            .query_id_into::<HtmlInputElement>()?
            .set_placeholder(&sspeeds.iter().join(" | "));

        "exposures-aperture-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");

        "exposures-lens-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");
        let lenses = contents
            .iter()
            .filter_map(|e| e.lens.as_deref())
            .collect::<BTreeSet<_>>();
        "exposures-lens-input"
            .query_id_into::<HtmlInputElement>()?
            .set_placeholder(&lenses.iter().join(" | "));

        "exposures-comment-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");
        let comments = contents
            .iter()
            .filter_map(|e| e.comment.as_deref())
            .collect::<BTreeSet<_>>();
        "exposures-comment-input"
            .query_id_into::<HtmlInputElement>()?
            .set_placeholder(&comments.iter().join(" | "));

        "exposures-date-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value("");

        let gps_input = "exposures-gps-input".query_id_into::<HtmlInputElement>()?;
        gps_input.set_value("");
        let positions = contents
            .iter()
            .filter_map(|e| e.gps.map(|(lat, lng)| format!("{lat}, {lng}")))
            .collect::<BTreeSet<_>>();

        // Set the input contents depending on the selection
        match positions.len() {
            // Empty if empty
            0 => (),
            // Hard if one singular location
            1 => gps_input.set_value(positions.first().map(String::as_str).unwrap_or_default()),
            // Placeholder if not
            _ => gps_input.set_placeholder("multiple"),
        }

        if !positions.is_empty() {
            map::show_location(&contents.iter().filter_map(|c| c.gps).collect::<Vec<_>>());
        } else {
            map::reset();
        }

        Ok(())
    }

    pub fn set_gps_input_contents(contents: &str) -> Result<(), Error> {
        "exposures-gps-input"
            .query_id_into::<HtmlInputElement>()?
            .set_value(contents);

        Ok(())
    }

    pub fn allow_undo(permission: bool) -> Result<(), Error> {
        let button = "undo".query_id()?;
        if permission {
            button.remove_attribute("disabled")?;
        } else {
            button.set_attribute("disabled", "1")?;
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
        view::{self, update},
    };
    use wasm_bindgen::prelude::*;
    use web_sys::{Event, HtmlInputElement};

    fn roll(format: impl Fn(String) -> UIRollUpdate + 'static + Clone) -> Closure<dyn Fn(Event)> {
        Closure::new(move |event: Event| {
            event
                .target_into::<HtmlInputElement>()
                .and_then(|t| controller::update(Update::Roll(format.clone()(t.value()))))
                .aquiesce()
        })
    }

    thread_local! {
    static UNDO: Closure<dyn Fn(Event)> = update(Update::Undo);
    static REORDER: Closure<dyn Fn(Event)> = Closure::new(|_| view::preview::contact_sheet().aquiesce());

    static RESET_EDITOR: Closure<dyn Fn(Event)> =
        Closure::new(|_| view::editor::reset().aquiesce());

    static EXPORT: Closure<dyn Fn(Event)> = Closure::new(|_| {
        wasm_bindgen_futures::spawn_local(async {
            crate::process_images().await.aquiesce();
        });
    });

    static UPDATE_AUTHOR: Closure<dyn Fn(Event)> = roll(UIRollUpdate::Author);
    static UPDATE_MAKE: Closure<dyn Fn(Event)> = roll(UIRollUpdate::Make);
    static UPDATE_MODEL: Closure<dyn Fn(Event)> = roll(UIRollUpdate::Model);
    static UPDATE_ISO: Closure<dyn Fn(Event)> = roll(UIRollUpdate::Iso);
    static UPDATE_DESCRIPTION: Closure<dyn Fn(Event)> = roll(UIRollUpdate::Film);
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
        "undo".query_id()?.on("click", &UNDO)?;
        "reorder".query_id()?.on("click", &REORDER)?;

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
