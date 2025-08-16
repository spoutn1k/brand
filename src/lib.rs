pub mod controller;
pub mod fs;
pub mod gps;
pub mod image_management;
pub mod macros;
pub mod models;

use crate::{
    macros::{
        MacroError, body, el, event_target, query_id, query_selector, roll_input, roll_placeholder,
        storage,
    },
    models::{Data, ExposureSpecificData, RollData},
};
use controller::{UIExposureUpdate, UIRollUpdate, Update};
use futures_lite::{AsyncReadExt, StreamExt};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};
use wasm_bindgen::prelude::*;
use web_sys::Blob;

pub type JsResult<T = ()> = Result<T, JsValue>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("JS failure: {0}")]
    Js(String),
    #[error(transparent)]
    ChannelSend(#[from] async_channel::SendError<ProcessStatus>),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Macro(#[from] crate::macros::MacroError),
    #[error("Missing key from storage: {0}")]
    MissingKey(String),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

impl From<JsValue> for Error {
    fn from(value: JsValue) -> Self {
        Error::Js(
            value
                .as_string()
                .unwrap_or_else(|| "Unknown JS error".to_string()),
        )
    }
}

impl From<Error> for JsValue {
    fn from(value: Error) -> Self {
        JsValue::from_str(&value.to_string())
    }
}

pub trait Aquiesce {
    fn aquiesce(self);
}

impl Aquiesce for Result<(), Error> {
    fn aquiesce(self) {
        if let Err(e) = self {
            log::error!("Error: {}", e);
        }
    }
}

pub enum ProcessStatus {
    Success,
    Error(String),
}

async fn process_exposure(
    metadata: &FileMetadata,
    s: async_channel::Sender<ProcessStatus>,
) -> Result<(), Error> {
    let path = PathBuf::from("originals").join(&metadata.name);
    let photo_data = web_fs::read(path).await?;

    web_fs::write(format!("exposure-{}.tif", metadata.index), &photo_data).await?;

    s.send(ProcessStatus::Success).await?;

    drop(s);

    Ok(())
}

async fn process_images() -> Result<(), Error> {
    let data: Vec<FileMetadata> = serde_json::from_str(
        &storage!()
            .get_item("metadata")?
            .ok_or(Error::MissingKey("metadata".into()))?,
    )?;

    let (s, r) = async_channel::unbounded();

    for entry in data {
        process_exposure(&entry, s.clone()).await?;
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
    let mut archive_builder = tar::Builder::new(Cursor::new(&mut archive));

    let mut stream = web_fs::read_dir(".").await?;
    while let Some(entry) = stream.next().await {
        let entry = entry?;

        if entry.file_type().await?.is_dir() {
            continue;
        }

        log::info!("Found entry: {:?}", entry);

        let mut file = web_fs::File::open(entry.path()).await?;
        let mut buffer = vec![];
        file.read_to_end(&mut buffer).await?;

        let mut header = tar::Header::new_gnu();
        header.set_size(buffer.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        archive_builder.append_data(
            &mut header,
            entry.path().file_name().ok_or(JsValue::from_str("wow"))?,
            Cursor::new(buffer),
        )?;
    }

    drop(archive_builder);

    let bytes = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(&archive) }.into());

    let array = js_sys::Array::new();
    array.push(&bytes.buffer());

    let props = web_sys::BlobPropertyBag::new();
    props.set_type("application/x-tar");

    let blob = Blob::new_with_u8_array_sequence_and_options(&array, &props)?;

    let element = el!("a", web_sys::HtmlElement);
    element.set_attribute("href", &web_sys::Url::create_object_url_with_blob(&blob)?)?;
    //element.set_attribute("download", &filename)?;
    element.style().set_property("display", "none")?;

    let body = body!();

    body.append_with_node_1(&element)?;
    element.click();
    body.remove_child(&element)?;

    Ok(())
}

fn extract_index_from_filename(filename: &str) -> Option<u32> {
    let re = regex::Regex::new(r"([0-9]+)").ok()?;
    re.captures(filename)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .map(|i| i % 100)
}

#[derive(Default, Debug, Serialize, Deserialize)]
enum Orientation {
    #[default]
    Normal,
    Rotated90,
    Rotated180,
    Rotated270,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct FileMetadata {
    name: String,
    index: u32,
    orientation: Orientation,
    file_type: Option<String>,
}

fn setup_editor_from_files(files: &[web_sys::FileSystemFileEntry]) -> Result<(), Error> {
    fill_roll_fields(&RollData::default())?;

    let metadata = files
        .iter()
        .map(|f: &web_sys::FileSystemFileEntry| FileMetadata {
            name: f.name(),
            index: extract_index_from_filename(&f.name()).unwrap_or(0),
            orientation: Orientation::Normal,
            file_type: Path::new(&f.name())
                .extension()
                .and_then(ImageFormat::from_extension)
                .map(|fmt| fmt.to_mime_type().to_string()),
        })
        .collect::<Vec<_>>();

    storage!().set_item("metadata", &serde_json::to_string(&metadata)?)?;

    let mut template = Data::default();
    for index in 1..=files.len() as u32 {
        template
            .exposures
            .insert(index, ExposureSpecificData::default());

        create_row(index, false)?;

        let image = el!("img");
        image.set_id(&format!("exposure-{index}-preview"));
        image.set_attribute("alt", &format!("E{}", index))?;

        query_id!("preview").append_with_node_1(&image)?;
    }

    storage!().set_item("data", &serde_json::to_string(&template)?)?;

    wasm_bindgen_futures::spawn_local(async move {
        web_fs::create_dir("originals")
            .await
            .map_err(|e| {
                log::error!("Failed to create originals directory: {e}");
            })
            .ok();
    });

    for entry in files {
        let name = entry.name();

        // We need to get the web_sys::File from the FileSystemFileEntry, so a closure is used,
        // then we create a FileReader to read the file, and add a callback to handle the file once
        // it's loaded. Finally, the file is written to the filesystem, in an async block.
        let file_load = Closure::once(move |file: web_sys::File| -> JsResult {
            let reader = web_sys::FileReader::new()?;

            let r = reader.clone();
            let value = file.name();
            let closure = Closure::once(move |_: web_sys::Event| {
                let mut path = PathBuf::from("originals");
                path.push(name);
                let p = path.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    fs::write_to_fs(p.as_path(), r).await.aquiesce();

                    controller::update(Update::ExposureImage(
                        extract_index_from_filename(&value).unwrap_or(0),
                        path.as_os_str().to_string_lossy().to_string(),
                    ))
                    .inspect_err(|e| {
                        log::error!(
                            "Failed to update exposure image: {}",
                            e.as_string().unwrap_or_default()
                        );
                    })
                    .ok();
                });
            });
            reader.set_onloadend(Some(closure.as_ref().unchecked_ref()));
            closure.forget();

            reader.read_as_array_buffer(&file)?;

            Ok(())
        });

        entry.file_with_callback(file_load.as_ref().unchecked_ref());
        file_load.forget();
    }

    query_id!("photoselect").class_list().add_1("hidden")?;
    query_id!("editor").class_list().remove_1("hidden")?;

    Ok(())
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
        fs::clear_dir("")
            .await
            .map_err(|e| {
                log::error!("Failed to clear directory: {e}");
            })
            .ok();
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

    let download_tse = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        wasm_bindgen_futures::spawn_local(async move {
            process_images()
                .await
                .inspect_err(|e| {
                    log::error!("Failed to process images: {e}");
                })
                .ok();
        });
    });

    query_id!("download")
        .add_event_listener_with_callback("click", download_tse.as_ref().unchecked_ref())?;
    download_tse.forget();

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

            setup_editor_from_files(&files)?;

            Ok(())
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

fn setup_editor_from_data(contents: &Data) -> JsResult {
    fill_roll_fields(&contents.roll)?;
    let selection = controller::get_selection()?;

    let mut exposures: Vec<(&u32, &ExposureSpecificData)> = contents.exposures.iter().collect();
    exposures.sort_by_key(|e| e.0);

    for (index, data) in exposures {
        create_row(*index, selection.contains(*index))?;
        controller::update(Update::Exposure(*index, data.clone()))?;

        //if let Err(e) = controller::update(Update::ExposureImageRestore(*index)) {
        //    log::debug!("{e:?}");
        //}
    }

    storage!().set_item(
        "data",
        &serde_json::to_string(&contents).map_err(|e| format!("{e}"))?,
    )?;

    query_id!("photoselect").class_list().add_1("hidden")?;
    query_id!("editor").class_list().remove_1("hidden")
}

pub fn setup() -> JsResult {
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

    wasm_bindgen_futures::spawn_local(async move {
        let mut fs_log = String::new();
        fs::print_dir_recursively("", 0, &mut fs_log).await;
        log::info!("{}", fs_log);
    });

    Ok(())
}
