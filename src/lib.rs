pub mod controller;
pub mod fs;
pub mod gps;
pub mod image_management;
pub mod macros;
pub mod models;

use crate::{
    macros::{
        MacroError, SessionStorageExt, body, el, event_target, query_id, query_selector,
        roll_input, roll_placeholder, storage,
    },
    models::{Data, ExposureSpecificData, FileMetadata, Meta, Orientation, RollData},
};
use controller::{UIExposureUpdate, UIRollUpdate, Update};
use futures_lite::StreamExt;
use image::ImageFormat;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};
use wasm_bindgen::prelude::*;
use web_sys::Blob;

pub type JsResult<T = ()> = Result<T, JsValue>;

#[wasm_bindgen]
extern "C" {
    fn set_marker(x: f64, y: f64);
    fn prompt_coords(i: u32);
    fn encodeURIComponent(i: String) -> String;
    #[wasm_bindgen(js_namespace = console)]
    pub fn log(s: &str);
}

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
    Macro(#[from] MacroError),
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

pub trait JsError {
    fn js_error(self) -> JsResult;
}

impl<E: std::error::Error> JsError for Result<(), E> {
    fn js_error(self) -> JsResult {
        self.map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

pub trait Aquiesce {
    fn aquiesce(self);
}

impl<E: std::error::Error> Aquiesce for Result<(), E> {
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
    let photo_data = web_fs::read(
        metadata
            .local_fs_path
            .clone()
            .ok_or(Error::MissingKey("Missing local file".into()))?,
    )
    .await?;

    let photo = image::ImageReader::new(Cursor::new(photo_data))
        .with_guessed_format()?
        .decode()?;

    let photo = match metadata.orientation {
        Orientation::Normal => photo,
        Orientation::Rotated90 => photo.rotate90(),
        Orientation::Rotated180 => photo.rotate180(),
        Orientation::Rotated270 => photo.rotate270(),
    };

    let data = controller::get_exposure_data(metadata.index)?;

    let mut output = vec![];
    output.reserve(2 * 1024 * 1024); // Reserve 2MB for the output

    image_management::encode_jpeg_with_exif(
        photo
            .clone()
            .resize(2000, 2000, image::imageops::FilterType::Lanczos3),
        Cursor::new(&mut output),
        &data,
    )
    .expect("Global error");

    web_fs::write(format!("{:0>2}.jpeg", metadata.index), output).await?;

    let mut output = vec![];
    output.reserve(50 * 1024 * 1024); // Reserve 50MB for the output

    image_management::encode_tiff_with_exif(photo, Cursor::new(&mut output), &data)
        .expect("Failed to encode TIFF with EXIF");

    web_fs::write(format!("{:0>2}.tiff", metadata.index), output).await?;

    log::info!("Exported to TIFF && JPEG successfully");

    s.send(ProcessStatus::Success).await?;

    Ok(())
}

static ARCHIVE_SIZE: usize = 2 * 1024 * 1024 * 1024; // 2GB

fn create_archive() -> tar::Builder<Cursor<Vec<u8>>> {
    //let archive = unsafe {
    //    let ptr = std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(
    //        2 * 1024 * 1024 * 1024,
    //        1,
    //    ));

    //    std::slice::from_raw_parts_mut(ptr, 2 * 1024 * 1024 * 1024)
    //};

    tar::Builder::new(Cursor::new(Vec::<u8>::with_capacity(ARCHIVE_SIZE)))
}

async fn process_images() -> Result<(), Error> {
    let data: Meta = serde_json::from_str(
        &storage!()
            .get_item("metadata")?
            .ok_or(Error::MissingKey("metadata".into()))?,
    )?;

    let (s, r) = async_channel::unbounded();

    for (_, entry) in data {
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

    {
        let mut archive_builder = create_archive();

        let mut archive_num = 1;
        let mut counter: usize = 0;
        let mut stream = web_fs::read_dir(".").await?;
        while let Some(entry) = stream.next().await {
            let entry = entry?;

            if entry.file_type().await?.is_dir() {
                continue;
            }

            let file = web_fs::read(entry.path()).await?;

            if counter + file.len() > ARCHIVE_SIZE {
                let archive_name = format!("roll_{archive_num}.tar");
                download_buffer(
                    archive_builder.into_inner()?.into_inner().as_slice(),
                    &archive_name,
                    "application/x-tar",
                )?;
                archive_num += 1;
                counter = 0;
                archive_builder = create_archive();
            }
            counter += file.len();

            log::info!(
                "Adding file {} to archive ({} bytes, {}MB total)",
                entry.path().display(),
                file.len(),
                counter / (1024 * 1024)
            );

            let mut header = tar::Header::new_gnu();
            header.set_size(file.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();

            archive_builder.append_data(
                &mut header,
                entry.path().file_name().ok_or(JsValue::from_str("wow"))?,
                Cursor::new(file),
            )?;
        }

        download_buffer(
            archive_builder.into_inner()?.into_inner().as_slice(),
            &format!("roll_{archive_num}.tar"),
            "application/x-tar",
        )?;
    }

    Ok(())
}

fn download_buffer(buffer: &[u8], filename: &str, mime_type: &str) -> Result<(), JsValue> {
    let bytes = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(&buffer) }.into());

    let array = js_sys::Array::new();
    array.push(&bytes.buffer());

    let props = web_sys::BlobPropertyBag::new();
    props.set_type(mime_type);

    let blob = Blob::new_with_u8_array_sequence_and_options(&array, &props)?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let element = el!("a", web_sys::HtmlElement);
    element.set_attribute("href", &url)?;
    element.set_attribute("download", filename)?;
    element.style().set_property("display", "none")?;

    body!().append_with_node_1(&element)?;
    element.click();
    body!().remove_child(&element)?;

    web_sys::Url::revoke_object_url(&url)?;

    Ok(())
}

fn extract_index_from_filename(filename: &str) -> Option<u32> {
    let re = regex::Regex::new(r"([0-9]+)").ok()?;
    re.captures(filename)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .map(|i| i % 100)
}

fn setup_editor_from_files(files: &[web_sys::FileSystemFileEntry]) -> Result<(), Error> {
    fill_roll_fields(&RollData::default())?;

    let metadata = files
        .iter()
        .map(|f: &web_sys::FileSystemFileEntry| {
            (
                PathBuf::from(f.name()),
                FileMetadata {
                    name: f.name(),
                    local_fs_path: None,
                    index: extract_index_from_filename(&f.name()).unwrap_or(0),
                    orientation: Orientation::Normal,
                    file_type: Path::new(&f.name())
                        .extension()
                        .and_then(ImageFormat::from_extension)
                        .map(|fmt| fmt.to_mime_type().to_string()),
                },
            )
        })
        .collect::<Vec<(PathBuf, FileMetadata)>>();

    storage!().set_item(
        "metadata",
        &serde_json::to_string(&metadata.iter().cloned().collect::<Meta>())?,
    )?;

    let mut template = Data::default();
    for index in 1..=files.len() as u32 {
        template
            .exposures
            .insert(index, ExposureSpecificData::default());

        create_row(index, false)?;
    }

    storage!().set_item("data", &serde_json::to_string(&template)?)?;

    wasm_bindgen_futures::spawn_local(async move {
        web_fs::create_dir("originals").await.aquiesce();
    });

    for ((file_id, mut meta), entry) in metadata.into_iter().zip(files) {
        let name = entry.name();

        // We need to get the web_sys::File from the FileSystemFileEntry, so a closure is used,
        // then we create a FileReader to read the file, and add a callback to handle the file once
        // it's loaded. Finally, the file is written to the filesystem, in an async block.
        let file_load = Closure::once(move |file: web_sys::File| -> JsResult {
            let reader = web_sys::FileReader::new()?;

            let r = reader.clone();
            let closure = Closure::once(move |_: web_sys::Event| {
                let path = PathBuf::from("originals").join(name);

                wasm_bindgen_futures::spawn_local(async move {
                    meta.local_fs_path = Some(path.clone());
                    fs::write_to_fs(&path, r).await.aquiesce();
                    controller::update(Update::FileMetadata(file_id, meta.clone()))
                        .await
                        .aquiesce();
                    controller::update(Update::ExposureImage(meta.index))
                        .await
                        .aquiesce();
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
    field: impl Fn(String) -> UIRollUpdate + 'static + Clone,
    input: &web_sys::Element,
) -> JsResult {
    let handler = Closure::<dyn Fn(_)>::new(move |event: web_sys::InputEvent| {
        let field = field.clone();
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::Roll(field(
                event
                    .target()
                    .map(|t| t.unchecked_into::<web_sys::HtmlInputElement>().value())
                    .unwrap_or_default(),
            )))
            .await
            .aquiesce()
        })
    });

    input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
    handler.forget();

    Ok(())
}

fn set_exposure_handler(
    index: u32,
    field: impl Fn(String) -> UIExposureUpdate + 'static + Clone,
    input: &web_sys::Element,
) -> JsResult {
    let handler = Closure::<dyn Fn(_)>::new(move |event: web_sys::InputEvent| {
        let field = field.clone();
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::ExposureField(
                index,
                field(
                    event
                        .target()
                        .map(|t| t.unchecked_into::<web_sys::HtmlInputElement>().value())
                        .unwrap_or_default(),
                ),
            ))
            .await
            .aquiesce()
        })
    });

    input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
    handler.forget();

    Ok(())
}

fn reset_editor() -> JsResult {
    query_id!("exposures").set_inner_html("");
    query_id!("preview").set_inner_html("");

    let selector = query_id!("photoselect", web_sys::HtmlInputElement);
    selector.class_list().remove_1("hidden")?;
    selector.set_value("");

    wasm_bindgen_futures::spawn_local(async move {
        fs::clear_dir("").await.aquiesce();
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

    let rotate_left = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::RotateLeft).await.aquiesce()
        });
    });
    query_id!("rotate-left")
        .add_event_listener_with_callback("click", rotate_left.as_ref().unchecked_ref())?;
    rotate_left.forget();

    let reset_editor =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            reset_editor()
        });
    query_id!("editor-reset")
        .add_event_listener_with_callback("click", reset_editor.as_ref().unchecked_ref())?;
    reset_editor.forget();

    let selection_clear = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::SelectionClear).await.aquiesce()
        });
    });
    query_id!("editor-selection-clear")
        .add_event_listener_with_callback("click", selection_clear.as_ref().unchecked_ref())?;
    selection_clear.forget();

    let selection_glob = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::SelectionAll).await.aquiesce()
        });
    });
    query_id!("editor-selection-glob")
        .add_event_listener_with_callback("click", selection_glob.as_ref().unchecked_ref())?;
    selection_glob.forget();

    let selection_invert = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::SelectionInvert).await.aquiesce()
        });
    });
    query_id!("editor-selection-invert")
        .add_event_listener_with_callback("click", selection_invert.as_ref().unchecked_ref())?;
    selection_invert.forget();

    let download_tse = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        wasm_bindgen_futures::spawn_local(async move {
            process_images().await.aquiesce();
        });
    });

    query_id!("download")
        .add_event_listener_with_callback("click", download_tse.as_ref().unchecked_ref())?;
    download_tse.forget();

    Ok(())
}

fn create_row(index: u32, selected: bool) -> Result<(), Error> {
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

    let select_action = Closure::<dyn Fn(_)>::new(move |e: web_sys::MouseEvent| {
        wasm_bindgen_futures::spawn_local(async move {
            controller::update(Update::SelectExposure(index, e.shift_key(), e.ctrl_key()))
                .await
                .aquiesce()
        });
    });
    row.add_event_listener_with_callback("click", select_action.as_ref().unchecked_ref())?;
    select_action.forget();

    let coords_select =
        Closure::<dyn Fn(_) -> JsResult>::new(move |_: web_sys::Event| -> JsResult {
            let data: Data = serde_json::from_str(&storage!().clone().get_existing("data")?)
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
    gps_select.add_event_listener_with_callback("click", coords_select.as_ref().unchecked_ref())?;
    coords_select.forget();

    select.append_with_node_1(&select_button)?;
    sspeed.append_with_node_1(&sspeed_input)?;
    aperture.append_with_node_1(&aperture_input)?;
    lens.append_with_node_1(&lens_input)?;
    comment.append_with_node_1(&comment_input)?;
    date.append_with_node_1(&date_input)?;
    gps.append_with_node_2(&gps_input, &gps_select)?;

    row.append_with_node_6(&select, &sspeed, &aperture, &lens, &comment, &date)?;
    row.append_with_node_1(&gps)?;
    table.append_with_node_1(&row)?;

    let image = el!("img");
    image.set_id(&format!("exposure-{index}-preview"));
    image.set_attribute("alt", &format!("E{}", index))?;
    query_id!("preview").append_with_node_1(&image)?;

    Ok(())
}

#[wasm_bindgen]
pub fn update_coords(index: u32, lat: f64, lon: f64) -> JsResult {
    wasm_bindgen_futures::spawn_local(async move {
        controller::update(Update::ExposureField(
            index,
            UIExposureUpdate::Gps(format!("{lat}, {lon}")),
        ))
        .await
        .aquiesce()
    });

    Ok(())
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

fn setup_editor_from_data(contents: Data) -> Result<(), Error> {
    fill_roll_fields(&contents.roll)?;
    let selection = controller::get_selection()?;

    storage!().set_item("data", &serde_json::to_string(&contents)?)?;
    let mut exposures: Vec<(u32, ExposureSpecificData)> = contents.exposures.into_iter().collect();
    exposures.sort_by_key(|e| e.0);

    for (index, _) in &exposures {
        create_row(*index, selection.contains(*index)).aquiesce();
    }

    wasm_bindgen_futures::spawn_local(async move {
        for (index, data) in exposures {
            controller::update(Update::Exposure(index, data.clone()))
                .await
                .aquiesce();
            controller::update(Update::ExposureImage(index))
                .await
                .aquiesce();
        }
    });

    query_id!("photoselect").class_list().add_1("hidden")?;
    query_id!("editor").class_list().remove_1("hidden")?;

    Ok(())
}

pub fn setup() -> JsResult {
    wasm_bindgen_futures::spawn_local(async move {
        let mut fs_log = String::new();
        fs::print_dir_recursively("", 0, &mut fs_log).await;
        log::info!("{}", fs_log);
    });

    let selector = query_id!("photoselect", web_sys::HtmlInputElement);
    disable_click(&selector)?;
    setup_drag_drop(&selector)?;
    setup_roll_fields()?;

    let storage = storage!();
    if let Some(data) = storage.get_item("data")? {
        let data: Data = serde_json::from_str(&data).map_err(|e| format!("{e}"))?;
        setup_editor_from_data(data).js_error()?;
    }

    Ok(())
}

#[wasm_bindgen]
pub fn clear() {
    wasm_bindgen_futures::spawn_local(async move {
        fs::clear_dir("").await.aquiesce();
    });
}

#[wasm_bindgen]
pub fn shared_memory() -> wasm_bindgen::JsValue {
    wasm_bindgen::memory()
}
