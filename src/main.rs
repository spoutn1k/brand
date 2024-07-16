use base64::prelude::*;
use image::io::Reader as ImageReader;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

mod models;
use models::{Data, ExposureSpecificData, RollData};

#[macro_use]
mod macros;

mod controller;
use controller::{UIExposureUpdate, UIRollUpdate, Update};

type JsResult<T = ()> = Result<T, JsValue>;

fn embed_file(photo_data: &[u8], target: &web_sys::Element) -> JsResult {
    let photo = ImageReader::new(std::io::Cursor::new(photo_data))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;

    let photo = photo.resize(256, 256, image::imageops::FilterType::Nearest);

    let mut jpg = vec![];
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpg);
    encoder.encode_image(&photo).map_err(|e| e.to_string())?;

    log::debug!("Size: {} -> {}", photo_data.len(), jpg.len());

    target.set_attribute(
        "src",
        &format!(
            "data:image/{};base64, {}",
            "jpeg",
            BASE64_STANDARD.encode(jpg)
        ),
    )
}

fn read_file(file: web_sys::File, target: web_sys::Element) -> JsResult {
    let reader = web_sys::FileReader::new()?;
    reader.read_as_array_buffer(&file)?;

    let r = reader.clone();
    let closure = Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
        let buffer = r.result()?;
        let data = js_sys::Uint8Array::new(&buffer);

        // Create a Rust slice from the Uint8Array
        embed_file(&data.to_vec(), &target.clone())
    });
    reader.set_onloadend(Some(&closure.as_ref().unchecked_ref()));
    closure.forget();

    let error_handler = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        log::error!("Failed to read file !");
    });
    reader.set_onerror(Some(&error_handler.as_ref().unchecked_ref()));
    error_handler.forget();

    Ok(())
}

fn setup_editor_from_files(files: &Vec<web_sys::FileSystemFileEntry>) -> JsResult {
    setup_roll_fields(&RollData::default())?;

    let mut index = HashMap::<u32, Vec<web_sys::FileSystemFileEntry>>::new();
    let re = regex::Regex::new(r"([0-9]+)").map_err(|e| e.to_string())?;
    let _ = files
        .into_iter()
        .map(|f| (String::from(f.name()), f.to_owned()))
        .filter_map(|(name, file): (String, web_sys::FileSystemFileEntry)| {
            match re.captures(&name) {
                Some(value) => {
                    let (index, _) = value.extract::<1>();
                    Some((String::from(index), file))
                }
                None => None,
            }
        })
        .for_each(|(index_str, file)| {
            str::parse::<u32>(&index_str)
                .and_then(|i| match index.get_mut(&i) {
                    Some(container) => Ok(container.push(file)),
                    None => {
                        index.insert(i, vec![file]);
                        Ok(())
                    }
                })
                .ok();
        });

    let mut index: Vec<(u32, Vec<web_sys::FileSystemFileEntry>)> = index.into_iter().collect();
    index.sort_by_key(|e| e.0);

    let mut template = Data::default();
    for (index, _) in index.iter() {
        template
            .exposures
            .insert(*index, ExposureSpecificData::default());
    }

    storage!().set_item(
        "data",
        &serde_json::to_string(&template).map_err(|e| format!("{e}"))?,
    )?;

    for (index, entries) in index.into_iter() {
        let entry = entries.first().ok_or("No entries")?.to_owned();

        create_row(index, false)?;
        let file_load =
            Closure::<dyn Fn(_) -> JsResult>::new(move |f: web_sys::File| -> JsResult {
                read_file(f, query_id!(&format!("exposure-{index}-preview")))
            });

        entry.file_with_callback(file_load.as_ref().unchecked_ref());
        file_load.forget();
    }

    query_id!("photoselect").class_list().add_1("hidden")?;
    query_id!("editor").class_list().remove_1("hidden")
}

fn set_roll_handler(
    field: impl Fn(String) -> UIRollUpdate + 'static,
    input: &web_sys::Element,
) -> JsResult {
    let handler =
        Closure::<dyn Fn(_) -> JsResult>::new(move |event: web_sys::InputEvent| -> JsResult {
            controller::update(Update::Roll(field(
                event_target!(event, web_sys::HtmlInputElement).value(),
            )))
        });

    input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
    handler.forget();

    Ok(())
}

fn set_exposure_handler(
    index: u32,
    field: impl Fn(String) -> UIExposureUpdate + 'static,
    input: &web_sys::Element,
) -> JsResult {
    let handler =
        Closure::<dyn Fn(_) -> JsResult>::new(move |event: web_sys::InputEvent| -> JsResult {
            controller::update(Update::ExposureField(
                index,
                field(event_target!(event, web_sys::HtmlInputElement).value()),
            ))
        });

    input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
    handler.forget();

    Ok(())
}

fn reset_editor() -> JsResult {
    query_id!("exposures").set_inner_html("");

    let selector = query_id!("photoselect", web_sys::HtmlInputElement);
    selector.class_list().remove_1("hidden")?;
    selector.set_value("");

    query_id!("editor").class_list().add_1("hidden")?;
    storage!().clear()
}

fn setup_roll_fields(data: &RollData) -> JsResult {
    let author_input = roll_input!(author, "Author", data);
    let make_input = roll_input!(make, "Camera Brand", data);
    let model_input = roll_input!(model, "Camera Model", data);
    let iso_input = roll_input!(iso, "ISO", data);
    let description_input = roll_input!(description, "Film type", data);

    set_roll_handler(UIRollUpdate::Author, &author_input)?;
    set_roll_handler(UIRollUpdate::Make, &make_input)?;
    set_roll_handler(UIRollUpdate::Model, &model_input)?;
    set_roll_handler(UIRollUpdate::ISO, &iso_input)?;
    set_roll_handler(UIRollUpdate::Film, &description_input)?;

    let reset_editor =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            reset_editor()
        });
    query_id!("editor-reset")
        .add_event_listener_with_callback("click", reset_editor.as_ref().unchecked_ref())?;
    reset_editor.forget();

    let selection_clear =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            controller::update(Update::SelectionClear)
        });
    query_id!("editor-selection-clear")
        .add_event_listener_with_callback("click", selection_clear.as_ref().unchecked_ref())?;
    selection_clear.forget();

    let selection_glob =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            controller::update(Update::SelectionAll)
        });
    query_id!("editor-selection-glob")
        .add_event_listener_with_callback("click", selection_glob.as_ref().unchecked_ref())?;
    selection_glob.forget();

    let selection_invert =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            controller::update(Update::SelectionInvert)
        });
    query_id!("editor-selection-invert")
        .add_event_listener_with_callback("click", selection_invert.as_ref().unchecked_ref())?;
    selection_invert.forget();

    let download_tse =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            let data: Data = serde_json::from_str(&storage!().get_item("data")?.ok_or("No data")?)
                .map_err(|e| e.to_string())?;

            download_file("index.tse".into(), data.to_string())
        });
    query_id!("download")
        .add_event_listener_with_callback("click", download_tse.as_ref().unchecked_ref())?;
    download_tse.forget();

    Ok(())
}

fn update_exposure_ui(index: u32, data: &UIExposureUpdate) -> JsResult {
    let (id, contents) = match data {
        UIExposureUpdate::ShutterSpeed(value) => (&format!("exposure-input-sspeed-{index}"), value),
        UIExposureUpdate::Aperture(value) => (&format!("exposure-input-aperture-{index}"), value),
        UIExposureUpdate::Comment(value) => (&format!("exposure-input-comment-{index}"), value),
        UIExposureUpdate::Date(value) => (&format!("exposure-input-date-{index}"), value),
        UIExposureUpdate::Lens(value) => (&format!("exposure-input-lens-{index}"), value),
        UIExposureUpdate::GPS(value) => (&format!("exposure-input-gps-{index}"), value),
    };

    query_id!(id, web_sys::HtmlInputElement).set_value(contents);
    Ok(())
}

fn set_exposure_selection(index: u32, selected: bool) -> JsResult {
    query_id!(
        &format!("exposure-input-select-{index}"),
        web_sys::HtmlInputElement
    )
    .set_checked(selected);

    let classes = query_id!(&format!("exposure-{index}")).class_list();
    if selected {
        classes.add_1("selected")
    } else {
        classes.remove_1("selected")
    }
}

fn create_row(index: u32, selected: bool) -> JsResult {
    let table = query_selector!("table#exposures");

    let row = el!("tr");
    row.set_id(&format!("exposure-{index}"));
    if selected {
        row.class_list().add_1("selected")?;
    }
    let select = el!("td", web_sys::HtmlElement);
    select.set_id(&format!("exposure-select-{index}"));
    select.style().set_property("display", "none")?;
    let icon = el!("td");
    icon.set_id(&format!("exposure-image-{index}"));
    let sspeed = el!("td");
    sspeed.set_id(&format!("exposure-field-sspeed-{index}"));
    let aperture = el!("td");
    aperture.set_id(&format!("exposure-field-aperture-{index}"));
    let lens = el!("td");
    lens.set_id(&format!("exposure-field-lens-{index}"));
    let comment = el!("td");
    comment.set_id(&format!("exposure-field-comment-{index}"));
    let date = el!("td");
    date.set_id(&format!("exposure-field-date-{index}"));
    let gps = el!("td");
    gps.set_id(&format!("exposure-field-gps-{index}"));

    let select_button = el!("input", web_sys::HtmlInputElement);
    select_button.set_attribute("type", "checkbox")?;
    if selected {
        select_button.set_attribute("checked", "checked")?;
    }
    select_button.set_id(&format!("exposure-input-select-{index}"));

    let sspeed_input = el!("input");
    sspeed_input.set_id(&format!("exposure-input-sspeed-{index}"));
    sspeed_input.set_attribute("placeholder", "Shutter Speed")?;
    let aperture_input = el!("input");
    aperture_input.set_id(&format!("exposure-input-aperture-{index}"));
    aperture_input.set_attribute("placeholder", "Aperture")?;
    let lens_input = el!("input");
    lens_input.set_id(&format!("exposure-input-lens-{index}"));
    lens_input.set_attribute("placeholder", "Focal length")?;
    let comment_input = el!("input");
    comment_input.set_id(&format!("exposure-input-comment-{index}"));
    comment_input.set_attribute("placeholder", "Title")?;
    let date_input = el!("input");
    date_input.set_id(&format!("exposure-input-date-{index}"));
    date_input.set_attribute("type", "datetime-local")?;
    date_input.set_attribute("step", "1")?;
    let gps_input = el!("input");
    gps_input.set_id(&format!("exposure-input-gps-{index}"));
    gps_input.set_attribute("placeholder", "GPS coordinates")?;
    let gps_select = el!("button");
    gps_select.set_inner_html("Map");

    set_exposure_handler(index, UIExposureUpdate::ShutterSpeed, &sspeed_input)?;
    set_exposure_handler(index, UIExposureUpdate::Aperture, &aperture_input)?;
    set_exposure_handler(index, UIExposureUpdate::Lens, &lens_input)?;
    set_exposure_handler(index, UIExposureUpdate::Comment, &comment_input)?;
    set_exposure_handler(index, UIExposureUpdate::Date, &date_input)?;
    set_exposure_handler(index, UIExposureUpdate::GPS, &gps_input)?;

    let select_action =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            controller::update(Update::SelectExposure(index))
        });
    icon.add_event_listener_with_callback("click", select_action.as_ref().unchecked_ref())?;
    select_action.forget();

    {
        let coords_select =
            Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
                let data: Data = serde_json::from_str(
                    &storage!()
                        .clone()
                        .get_item("data")?
                        .ok_or("No data found !")?,
                )
                .map_err(|e| e.to_string())?;

                if let Some((lat, lon)) = data
                    .exposures
                    .get(&index)
                    .ok_or("Failed to access exposure")?
                    .gps
                {
                    set_marker(lat, lon);
                }

                prompt_coords(index);

                Ok(())
            });
        gps_select
            .add_event_listener_with_callback("click", coords_select.as_ref().unchecked_ref())?;
        coords_select.forget();
    }

    select.append_with_node_1(&select_button)?;
    sspeed.append_with_node_1(&sspeed_input)?;
    aperture.append_with_node_1(&aperture_input)?;
    lens.append_with_node_1(&lens_input)?;
    comment.append_with_node_1(&comment_input)?;
    date.append_with_node_1(&date_input)?;
    gps.append_with_node_2(&gps_input, &gps_select)?;

    let image = el!("img");
    image.set_id(&format!("exposure-{index}-preview"));
    image.set_attribute("alt", &format!("Exposure number {}", index))?;
    icon.append_with_node_1(&image)?;

    row.append_with_node_7(&select, &icon, &sspeed, &aperture, &lens, &comment, &date)?;
    row.append_with_node_1(&gps)?;
    table.append_with_node_1(&row)
}

#[wasm_bindgen]
pub fn update_coords(index: u32, lat: f64, lon: f64) -> JsResult {
    controller::update(Update::ExposureField(
        index,
        UIExposureUpdate::GPS(format!("{lat}, {lon}")),
    ))
}

fn setup_drag_drop(photo_selector: &web_sys::HtmlInputElement) -> JsResult {
    let closure =
        Closure::<dyn Fn(_) -> JsResult>::new(move |event: web_sys::InputEvent| -> JsResult {
            let files = event_target!(event, web_sys::HtmlInputElement)
                .webkit_entries()
                .iter()
                .map(|f| f.dyn_into::<web_sys::FileSystemFileEntry>())
                .collect::<Result<Vec<_>, _>>()?;

            setup_editor_from_files(&files)
        });

    photo_selector.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}

fn disable_click(selector: &web_sys::HtmlInputElement) -> JsResult {
    let disable_click =
        Closure::<dyn Fn(_)>::new(move |e: web_sys::InputEvent| e.prevent_default());
    selector.add_event_listener_with_callback("click", disable_click.as_ref().unchecked_ref())?;
    disable_click.forget();

    Ok(())
}

#[wasm_bindgen]
extern "C" {
    fn set_marker(x: f64, y: f64);
    fn prompt_coords(i: u32);
    fn encodeURIComponent(i: String) -> String;
}

fn download_file(filename: String, contents: String) -> JsResult {
    let element = el!("a", web_sys::HtmlElement);
    element.set_attribute(
        "href",
        &format!(
            "data:text/plain;charset=utf-8,{}",
            encodeURIComponent(contents)
        ),
    )?;
    element.set_attribute("download", &filename)?;
    element.style().set_property("display", "none")?;

    let body = web_sys::window()
        .ok_or("No window")?
        .document()
        .ok_or("no document on window")?
        .body()
        .ok_or("no body")?;

    body.append_with_node_1(&element)?;
    element.click();
    body.remove_child(&element)?;

    Ok(())
}

fn setup_editor_from_data(contents: &Data) -> JsResult {
    setup_roll_fields(&contents.roll)?;
    let selection: Vec<u32> =
        serde_json::from_str(&storage!().get_item("selected")?.unwrap_or("[]".into()))
            .map_err(|e| e.to_string())?;

    let mut exposures: Vec<(&u32, &ExposureSpecificData)> = contents.exposures.iter().collect();
    exposures.sort_by_key(|e| e.0);

    for (index, data) in exposures {
        create_row(*index, selection.contains(index))?;
        controller::update(Update::Exposure(*index, data.clone()))?;
    }

    storage!().set_item(
        "data",
        &serde_json::to_string(&contents).map_err(|e| format!("{e}"))?,
    )?;

    query_id!("photoselect").class_list().add_1("hidden")?;
    query_id!("editor").class_list().remove_1("hidden")
}

fn main() -> JsResult {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    let selector = query_id!("photoselect", web_sys::HtmlInputElement);
    disable_click(&selector)?;
    setup_drag_drop(&selector)?;

    let storage = storage!();
    if let Some(data) = storage.get_item("data")? {
        let data: Data = serde_json::from_str(&data).map_err(|e| format!("{e}"))?;
        if let Err(e) = setup_editor_from_data(&data) {
            log::error!("{e:?}");
        }
    }

    Ok(())
}
