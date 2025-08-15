use base64::prelude::*;
use image::{ImageReader, codecs::jpeg::JpegEncoder};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[macro_use]
mod macros;
mod controller;
mod models;

use controller::{UIExposureUpdate, UIRollUpdate, Update};
use futures_lite::AsyncReadExt;
use futures_lite::AsyncWriteExt;
use futures_lite::StreamExt;
use image::imageops::FilterType;
use models::{Data, ExposureSpecificData, RollData};
use std::sync::mpsc;
use web_fs::{create_dir, create_dir_all};
use web_sys::Blob;

type JsResult<T = ()> = Result<T, JsValue>;

enum ProcessStatus {
    Success,
    Error(String),
}

fn process_exposure(
    index: u32,
    file: web_sys::File,
    s: async_channel::Sender<ProcessStatus>,
) -> JsResult {
    let reader = web_sys::FileReader::new()?;

    let r = reader.clone();
    let closure = Closure::once(move |_: web_sys::Event| {
        let photo_data = js_sys::Uint8Array::new(&r.result().unwrap()).to_vec();

        wasm_bindgen_futures::spawn_local(async move {
            web_fs::File::create(format!("exposure-{index}.tif"))
                .await
                .unwrap()
                .write_all(&photo_data)
                .await
                .unwrap();

            s.send(ProcessStatus::Success).await.unwrap();

            drop(s);
        });
    });
    reader.set_onloadend(Some(closure.as_ref().unchecked_ref()));
    closure.forget();

    let error_handler = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        log::error!("Failed to read file !");
    });
    reader.set_onerror(Some(error_handler.as_ref().unchecked_ref()));
    error_handler.forget();

    reader.read_as_array_buffer(&file)?;

    Ok(())
}

async fn process_images() -> JsResult {
    let images = get_handles();

    let data: Data = serde_json::from_str(&storage!().get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    let (s, r) = async_channel::unbounded();

    for (index, entry) in images.into_iter() {
        let s = s.clone();
        let file_load = Closure::once(move |f: web_sys::File| -> JsResult {
            process_exposure(index.clone(), f, s.clone());
            Ok(())
        });
        entry.file_with_callback(file_load.as_ref().unchecked_ref());
        file_load.forget();
    }

    drop(s);

    while let Ok(status) = r.recv().await {
        match status {
            ProcessStatus::Success => {
                log::info!("Exposure processed successfully ({})", r.sender_count())
            }
            ProcessStatus::Error(e) => log::error!("Error processing exposure: {e}"),
        }
    }

    log::info!("All exposures processed");

    let mut archive = vec![];
    let mut archive_builder = tar::Builder::new(std::io::Cursor::new(&mut archive));

    let mut stream = web_fs::read_dir(".").await.unwrap();
    while let Some(entry) = stream.next().await {
        let entry = entry.unwrap();
        log::info!("Found entry: {:?}", entry);

        let mut file = web_fs::File::open(entry.path()).await.unwrap();
        let mut buffer = vec![];
        file.read_to_end(&mut buffer).await.unwrap();

        let mut header = tar::Header::new_gnu();
        header.set_size(buffer.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        archive_builder
            .append_data(
                &mut header,
                entry.path().file_name().unwrap(),
                std::io::Cursor::new(buffer),
            )
            .unwrap();
    }

    drop(archive_builder);

    let bytes = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(&archive) }.into());

    let array = js_sys::Array::new();
    array.push(&bytes.buffer());
    let blob = Blob::new_with_u8_array_sequence_and_options(
        &array,
        web_sys::BlobPropertyBag::new().type_("application/x-tar"),
    )
    .unwrap();
    let download_url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    log::info!("Download URL: {}", download_url);

    let element = el!("a", web_sys::HtmlElement);
    element.set_attribute("href", &download_url)?;
    //element.set_attribute("download", &filename)?;
    element.style().set_property("display", "none")?;

    let body = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .body()
        .unwrap();

    body.append_with_node_1(&element)?;
    element.click();
    body.remove_child(&element)?;

    Ok(())
}

fn read_file(index: u32, file: web_sys::File) -> JsResult {
    let reader = web_sys::FileReader::new()?;
    reader.read_as_array_buffer(&file)?;

    let r = reader.clone();
    let closure = Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
        let photo_data = js_sys::Uint8Array::new(&r.result()?).to_vec();

        // Create a Rust slice from the Uint8Array
        let photo = ImageReader::new(std::io::Cursor::new(photo_data))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .decode()
            .map_err(|e| e.to_string())?;

        let photo = photo.resize(512, 512, FilterType::Nearest);
        let mut jpg = vec![];
        JpegEncoder::new(&mut jpg)
            .encode_image(&photo)
            .map_err(|e| e.to_string())?;

        controller::update(Update::ExposureImage(index, BASE64_STANDARD.encode(jpg)))
    });
    reader.set_onloadend(Some(closure.as_ref().unchecked_ref()));
    closure.forget();

    let error_handler = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        log::error!("Failed to read file !");
    });
    reader.set_onerror(Some(error_handler.as_ref().unchecked_ref()));
    error_handler.forget();

    Ok(())
}

fn extract_index_from_filename(filename: &str) -> Option<u32> {
    let re = regex::Regex::new(r"([0-9]+)").ok()?;
    re.captures(filename)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .map(|i| i % 100)
}

fn setup_editor_from_files(files: &[web_sys::FileSystemFileEntry]) -> JsResult {
    fill_roll_fields(&RollData::default())?;

    let mut index = HashMap::<u32, web_sys::FileSystemFileEntry>::new();
    files
        .into_iter()
        .filter_map(|f: &web_sys::FileSystemFileEntry| {
            extract_index_from_filename(&f.name()).map(|i| (i, f.to_owned()))
        })
        .for_each(|(index_str, file)| {
            index.insert(index_str, file);
        });

    set_handles(&index);

    let mut template = Data::default();
    for (index, _) in index.iter() {
        template
            .exposures
            .insert(*index, ExposureSpecificData::default());
    }

    storage!().set_item(
        "data",
        &serde_json::to_string(&template).map_err(|e| e.to_string())?,
    )?;

    for (index, entry) in index.into_iter() {
        create_row(index, false)?;
        let file_load =
            Closure::<dyn Fn(_) -> JsResult>::new(move |f: web_sys::File| -> JsResult {
                read_file(index, f)
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

    wasm_bindgen_futures::spawn_local(async move {
        let mut root = web_fs::read_dir(".").await.unwrap();
        while let Some(entry) = root.next().await {
            let path = entry.unwrap().path();
            web_fs::remove_file(&path.clone()).await.ok();
        }
    });

    query_id!("editor").class_list().add_1("hidden")?;
    storage!().clear()
}

fn fill_roll_fields(data: &RollData) -> JsResult {
    roll_input!(author, data);
    roll_input!(make, data);
    roll_input!(model, data);
    roll_input!(iso, data);
    roll_input!(description, data);

    Ok(())
}

fn setup_roll_fields() -> JsResult {
    let author_input = roll_placeholder!(author, "Author");
    let make_input = roll_placeholder!(make, "Camera Brand");
    let model_input = roll_placeholder!(model, "Camera Model");
    let iso_input = roll_placeholder!(iso, "ISO");
    let description_input = roll_placeholder!(description, "Film type");

    set_roll_handler(UIRollUpdate::Author, &author_input)?;
    set_roll_handler(UIRollUpdate::Make, &make_input)?;
    set_roll_handler(UIRollUpdate::Model, &model_input)?;
    set_roll_handler(UIRollUpdate::Iso, &iso_input)?;
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

            //download_file("index.tse".into(), data.to_string())
            wasm_bindgen_futures::spawn_local(async move {
                process_images().await;
            });

            Ok(())
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
        UIExposureUpdate::Gps(value) => (&format!("exposure-input-gps-{index}"), value),
    };

    query_id!(id, web_sys::HtmlInputElement).set_value(contents);
    Ok(())
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
    set_exposure_handler(index, UIExposureUpdate::Gps, &gps_input)?;

    let select_action =
        Closure::<dyn Fn(_) -> JsResult>::new(move |e: web_sys::MouseEvent| -> JsResult {
            controller::update(Update::SelectExposure(index, e.shift_key(), e.ctrl_key()))
        });
    row.add_event_listener_with_callback("click", select_action.as_ref().unchecked_ref())?;
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

    row.append_with_node_6(&select, &sspeed, &aperture, &lens, &comment, &date)?;
    row.append_with_node_1(&gps)?;
    table.append_with_node_1(&row)
}

#[wasm_bindgen]
pub fn update_coords(index: u32, lat: f64, lon: f64) -> JsResult {
    controller::update(Update::ExposureField(
        index,
        UIExposureUpdate::Gps(format!("{lat}, {lon}")),
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

    fn get_raw_handles() -> js_sys::Map;
    fn set_raw_handles(h: js_sys::Map);
}

fn set_handles(handles: &HashMap<u32, web_sys::FileSystemFileEntry>) {
    let raw = js_sys::Map::new();
    for (key, value) in handles {
        raw.set(&serde_wasm_bindgen::to_value(&key).unwrap(), &value.into());
    }

    set_raw_handles(raw);
}

fn get_handles() -> HashMap<u32, web_sys::FileSystemFileEntry> {
    let raw = get_raw_handles();

    let mut handles = HashMap::new();
    for vec in raw.entries() {
        if let Ok(vec) = vec {
            let vec: Vec<JsValue> = vec.unchecked_into::<js_sys::Array>().to_vec();
            let key: u32 = vec
                .get(0)
                .and_then(|i| serde_wasm_bindgen::from_value(i.clone()).ok())
                .unwrap();
            let entry: web_sys::FileSystemFileEntry = vec
                .get(1)
                .map(|v| v.clone().unchecked_into::<web_sys::FileSystemFileEntry>())
                .expect(&format!("Failed to convert {key} to FileSystemFileEntry"));
            handles.insert(key, entry);
        }
    }

    handles
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
    fill_roll_fields(&contents.roll)?;
    let selection = controller::get_selection()?;

    let mut exposures: Vec<(&u32, &ExposureSpecificData)> = contents.exposures.iter().collect();
    exposures.sort_by_key(|e| e.0);

    for (index, data) in exposures {
        create_row(*index, selection.contains(*index))?;
        controller::update(Update::Exposure(*index, data.clone()))?;

        if let Err(e) = controller::update(Update::ExposureImageRestore(*index)) {
            log::debug!("{e:?}");
        }
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
    console_log::init_with_level(log::Level::Debug).unwrap();
    //    wasm_logger::init(wasm_logger::Config::default());
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

    let selector = query_id!("photoselect", web_sys::HtmlInputElement);
    disable_click(&selector)?;
    setup_drag_drop(&selector)?;
    setup_roll_fields()?;

    let storage = storage!();
    if let Some(data) = storage.get_item("data")? {
        let data: Data = serde_json::from_str(&data).map_err(|e| format!("{e}"))?;
        if let Err(e) = setup_editor_from_data(&data) {
            log::error!("{e:?}");
        }
    }

    Ok(())
}
