mod archive;
pub mod controller;
mod error;
pub mod fs;
pub mod gps;
mod helpers;
pub mod image_management;
pub mod models;
pub mod view;
pub mod worker;

use crate::{
    controller::get_exposure_data,
    models::{Data, FileKind, FileMetadata, Meta, Orientation, WorkerMessage},
};
use controller::Update;
use image::ImageFormat;
use js_sys::{Array, Uint8Array};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};
use wasm_bindgen::prelude::*;
use web_sys::{
    Blob, Event, File as WebFile, FileSystemFileEntry, HtmlElement, KeyEvent, KeyboardEvent,
    MessageEvent,
};

pub use error::{Aquiesce, Error, JsError, JsResult};
pub use helpers::{
    AsHtmlExt, EventTargetExt, QueryExt, SessionStorageExt, SetEventHandlerExt, body, document,
    storage,
};

pub mod bindings {
    use crate::{
        JsError, JsResult, controller,
        controller::{UIExposureUpdate, Update},
    };
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen]
    extern "C" {
        fn set_marker(x: f64, y: f64);
        pub fn prompt_coords();
    }

    #[wasm_bindgen]
    pub async fn update_coords(lat: f64, lon: f64) -> JsResult {
        controller::update(Update::Exposure(UIExposureUpdate::Gps(format!(
            "{lat}, {lon}"
        ))))
        .js_error()
    }
}

thread_local! {
static KEY_HANDLER: Closure<dyn Fn(KeyboardEvent)> = Closure::new(|e: KeyboardEvent|{
    if e.key_code() == KeyEvent::DOM_VK_ESCAPE {
        controller::update(controller::Update::SelectionClear).aquiesce()
    }
});
}

async fn process_images() -> Result<(), Error> {
    let data: Meta = serde_json::from_str(
        &storage()?
            .get_item("metadata")?
            .ok_or(Error::MissingKey("metadata".into()))?,
    )?;

    let folder_name = controller::generate_folder_name().unwrap_or_else(|e| {
        log::error!("Failed to generate folder name: {e:?}");
        "roll".to_string()
    });

    let exposures = get_exposure_data()?;

    let tasks: Vec<_> = data
        .into_values()
        .map(|entry| {
            WorkerMessage::Process(entry.clone(), Box::new(exposures.generate(entry.index)))
        })
        .collect();

    controller::notifier()
        .send(controller::Progress::ProcessingStart(tasks.len() as u32))
        .await?;

    let pool = worker::Pool::try_new_with_callback(
        tasks,
        Box::new(|_| {
            wasm_bindgen_futures::spawn_local(async move {
                controller::notifier()
                    .send(controller::Progress::Processing(0))
                    .await
                    .aquiesce();
            })
        }),
    )?;

    pool.join().await?;

    controller::notifier()
        .send(controller::Progress::ProcessingDone)
        .await?;

    archive::export_dir(".", folder_name.into()).await
}

fn download_buffer(buffer: &[u8], filename: &str, mime_type: &str) -> Result<(), Error> {
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

fn extract_index_from_filename(filename: &str) -> Option<u32> {
    let re = regex::Regex::new(r"([0-9]+)").ok()?;
    re.captures(filename)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .map(|i| i % 100)
}

async fn import_tse(entry: &FileSystemFileEntry) -> Result<(), Error> {
    let file_load = Closure::once(move |file: WebFile| -> JsResult {
        let reader = web_sys::FileReader::new()?;

        let r = reader.clone();
        let closure = Closure::once(move |_: Event| -> JsResult {
            let raw = r.result()?.as_string().unwrap_or_default();
            let data = models::read_tse(Cursor::new(raw))?;

            storage()?.set_item("data", &serde_json::to_string(&data).unwrap())?;

            Ok(())
        });

        reader.set_onloadend(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
        reader.read_as_text(&file)?;

        Ok(())
    });

    entry.file_with_callback(file_load.as_ref().unchecked_ref());
    file_load.forget();

    Ok(())
}

async fn setup_editor_from_files(files: &[FileSystemFileEntry]) -> Result<(), Error> {
    let (images, other): (Vec<_>, Vec<_>) = files
        .iter()
        .partition(|f| matches!(FileKind::from(PathBuf::from(&f.name())), FileKind::Image(_)));

    let metadata = images
        .iter()
        .map(|f| {
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

    let mut selected = std::collections::BTreeMap::new();
    for ((p, m), i) in metadata.into_iter().zip(images) {
        selected
            .entry(m.index)
            .and_modify(|((pi, mi), ii)| {
                if m.file_type.as_ref().is_some_and(|t| t == "image/tiff") {
                    *pi = p.clone();
                    *mi = m.clone();
                    *ii = i;
                }
            })
            .or_insert(((p, m), i));
    }

    let mut metadata = vec![];
    let mut images = vec![];

    for (_, (m, i)) in selected {
        metadata.push(m);
        images.push(i);
    }

    storage()?.set_item(
        "metadata",
        &serde_json::to_string(&metadata.iter().cloned().collect::<Meta>())?,
    )?;

    view::preview::create(images.len() as u32)?;

    storage()?.set_item(
        "data",
        &serde_json::to_string(&Data::with_count(images.len() as u32))?,
    )?;

    if let Some(file) = other
        .into_iter()
        .find(|f| matches!(FileKind::from(PathBuf::from(&f.name())), FileKind::Tse))
        .cloned()
    {
        import_tse(&file).await?
    }

    web_fs::create_dir("originals").await?;
    let earlier = instant::SystemTime::now();

    let (tx, rx) = async_channel::unbounded();
    for ((file_id, mut meta), entry) in metadata.iter().cloned().zip(images) {
        let name = entry.name();

        let tx = tx.clone();

        // We need to get the web_sys::File from the FileSystemFileEntry, so a closure is used,
        // then we create a FileReader to read the file, and add a callback to handle the file once
        // it's loaded. Finally, the file is written to the filesystem, in an async block.
        let file_load = Closure::once(move |file: web_sys::File| -> JsResult {
            let reader = web_sys::FileReader::new()?;

            let r = reader.clone();

            let closure = Closure::once(move |_: Event| {
                let path = PathBuf::from("originals").join(name);

                wasm_bindgen_futures::spawn_local(async move {
                    meta.local_fs_path = Some(path.clone());
                    fs::write_to_fs(&path, r).await.aquiesce();
                    controller::update(Update::FileMetadata(file_id, meta.clone())).aquiesce();
                    tx.send(()).await.unwrap();
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

    drop(tx);

    while rx.sender_count() > 0 {
        rx.recv().await?;
    }

    let now = instant::SystemTime::now()
        .duration_since(earlier)
        .unwrap_or_default()
        .as_secs_f32();

    log::debug!("Imported files in {now}s");

    "landing".query_id()?.class_list().add_1("hidden")?;
    "editor".query_id()?.class_list().remove_1("hidden")?;

    generate_thumbnails().await?;

    Ok(())
}

fn set_image(data: JsValue) -> Result<(), Error> {
    let models::WorkerCompressionAnswer(index, base64): models::WorkerCompressionAnswer =
        serde_wasm_bindgen::from_value(data)?;

    format!("exposure-{index}-preview")
        .query_id()?
        .set_attribute("src", &format!("data:image/jpeg;base64, {base64}"))?;

    wasm_bindgen_futures::spawn_local(async move {
        controller::notifier()
            .send(controller::Progress::ThumbnailGenerated(index))
            .await
            .aquiesce();
    });

    Ok(())
}

async fn setup_editor_from_data(contents: Data) -> Result<(), Error> {
    view::roll::fill_fields(&contents.roll)?;

    storage()?.set_item("data", &serde_json::to_string(&contents)?)?;

    view::preview::create(contents.exposures.len() as u32)?;

    view::landing::hide()?;
    view::editor::show()?;

    generate_thumbnails().await
}

async fn generate_thumbnails() -> Result<(), Error> {
    let data: Meta = serde_json::from_str(&storage()?.get_existing("metadata")?)?;
    let mut tasks: Vec<_> = data.into_values().collect();
    tasks.sort_by(|a, b| b.index.cmp(&a.index));

    let tasks: Vec<_> = tasks
        .into_iter()
        .map(models::WorkerMessage::GenerateThumbnail)
        .collect();

    controller::notifier()
        .send(controller::Progress::ThumbnailStart(tasks.len() as u32))
        .await?;

    let pool = worker::Pool::try_new_with_callback(
        tasks,
        Box::new(|e: MessageEvent| {
            set_image(e.data()).aquiesce();
        }),
    )?;

    pool.join().await?;

    controller::notifier()
        .send(controller::Progress::ThumbnailDone)
        .await?;

    Ok(())
}

#[wasm_bindgen]
pub fn setup() -> JsResult {
    view::preview::setup()
        .and(view::landing::setup())
        .and(view::exposure::setup())
        .and(view::roll::setup())?;
    controller::update(Update::SelectionClear).aquiesce();

    wasm_bindgen_futures::spawn_local(async { controller::handle_progress().await.aquiesce() });

    if let Some(data) = storage()?.get_item("data")? {
        let data: Data = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        wasm_bindgen_futures::spawn_local(
            async move { setup_editor_from_data(data).await.aquiesce() },
        );
    }

    // Listen for keypresses and handle them accordingly
    document()?.on("keydown", &KEY_HANDLER)?;

    wasm_bindgen_futures::spawn_local(async { view::landing::landing_stats().await.aquiesce() });

    Ok(())
}
