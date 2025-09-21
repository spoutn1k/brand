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
    error::IntoError,
    models::{Data, FileKind, FileMetadata, Orientation},
    worker::{WorkerCompressionAnswer, WorkerMessage, WorkerProcessingAnswer},
};
use controller::Update;
use futures::{
    SinkExt, StreamExt,
    channel::{mpsc, oneshot},
};
use std::{collections::BTreeMap, io::Cursor, path::PathBuf};
use wasm_bindgen::prelude::*;
use web_sys::{Event, File as WebFile, FileSystemFileEntry, KeyEvent, KeyboardEvent, MessageEvent};

pub use error::{Aquiesce, Error, JsError, JsResult};
pub use helpers::{
    AsHtmlExt, EventTargetExt, QueryExt, SessionStorageExt, SetEventHandlerExt, body, document,
};

thread_local! {
static KEY_HANDLER: Closure<dyn Fn(KeyboardEvent)> = Closure::new(|e: KeyboardEvent|{
    if e.key_code() == KeyEvent::DOM_VK_ESCAPE {
        controller::update(controller::Update::SelectionClear).aquiesce()
    }
});

}

fn handle_finished_export(
    event: MessageEvent,
    mut sender: futures::channel::mpsc::Sender<PathBuf>,
) -> Result<(), Error> {
    let result: WorkerProcessingAnswer = serde_wasm_bindgen::from_value(event.data())?;

    wasm_bindgen_futures::spawn_local(async move {
        for file in result.0 {
            sender.send(file).await.aquiesce();
        }

        controller::notify(controller::Progress::Processing(0))
            .await
            .aquiesce();
    });

    Ok(())
}

async fn process_images() -> Result<(), Error> {
    let metadata = controller::get_metadata()?;

    let folder_name = controller::generate_folder_name().unwrap_or_else(|e| {
        log::error!("Failed to generate folder name: {e:?}");
        "roll".to_string()
    });

    let exposures = controller::get_data().map(Data::spread_shots)?;

    let (sender, receiver) = mpsc::channel(80);
    let (ack, ok) = oneshot::channel();

    wasm_bindgen_futures::spawn_local(async move {
        archive::builder(folder_name.into(), receiver, ack)
            .await
            .aquiesce();
    });

    let tasks: Vec<_> = metadata
        .into_values()
        .map(|entry| {
            WorkerMessage::Process(entry.clone(), Box::new(exposures.generate(entry.index)))
        })
        .collect();

    controller::notify(controller::Progress::ProcessingStart(tasks.len() as u32)).await?;

    worker::Pool::try_new_with_callback(tasks, move |event| {
        handle_finished_export(event, sender.clone()).aquiesce();
    })?
    .join()
    .await?;

    ok.await?;

    controller::notify(controller::Progress::ProcessingDone).await?;

    fs::clear_dir("processed").await
}

fn extract_index_from_filename(filename: &str) -> Option<u32> {
    let re = regex::Regex::new(r"([0-9]+)").ok()?;
    re.captures(filename)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .map(|i| i % 100)
}

async fn import_tse(pipe: oneshot::Sender<Data>, entry: &FileSystemFileEntry) -> Result<(), Error> {
    let file_load = Closure::once(move |file: WebFile| -> JsResult {
        let reader = web_sys::FileReader::new()?;

        let r = reader.clone();
        let closure = Closure::once(move |_: Event| -> JsResult {
            let raw = r.result()?.as_string().unwrap_or_default();
            let data = models::read_tse(Cursor::new(raw))?;

            controller::set_data(&data)?;
            pipe.send(data).map_err(|_| Error::OsChannelSend).js_error()
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

async fn import_files(
    metadata: Vec<(PathBuf, FileMetadata)>,
    images: &[&FileSystemFileEntry],
) -> mpsc::Receiver<Result<(), Error>> {
    let (sender, rx) = mpsc::channel(80);

    for ((path, mut meta), entry) in metadata.iter().cloned().zip(images) {
        let name = entry.name();

        let mut tx = sender.clone();

        // We need to get the web_sys::File from the FileSystemFileEntry, so a closure is used, then we create a FileReader to read the file, and add a callback to handle the file once it's loaded. Finally, the file is written to the filesystem, in an async block.
        let file_load = Closure::once(move |file: web_sys::File| -> JsResult {
            let reader = web_sys::FileReader::new()?;

            let r = reader.clone();
            let closure = Closure::once(move |_: Event| {
                wasm_bindgen_futures::spawn_local(async move {
                    let local_path = PathBuf::from("originals").join(name);
                    meta.local_fs_path = local_path.clone();

                    let res = fs::write_to_fs(&local_path, r)
                        .await
                        .error()
                        .and(controller::update(Update::FileMetadata(path, meta)).error());
                    tx.send(res).await.aquiesce()
                })
            });
            reader.set_onloadend(Some(closure.as_ref().unchecked_ref()));
            closure.forget();

            reader.read_as_array_buffer(&file)
        });

        entry.file_with_callback(file_load.as_ref().unchecked_ref());
        file_load.forget();
    }

    drop(sender);

    rx
}

async fn setup_editor_from_files(files: &[FileSystemFileEntry]) -> Result<(), Error> {
    let (images, other): (Vec<_>, Vec<_>) = files
        .iter()
        .partition(|f| matches!(FileKind::from(PathBuf::from(f.name())), FileKind::Image(_)));

    let metadata = images
        .iter()
        .map(|f| {
            (
                PathBuf::from(f.name()),
                FileMetadata {
                    name: f.name(),
                    local_fs_path: "".into(),
                    index: extract_index_from_filename(&f.name()).unwrap_or(0),
                    orientation: Orientation::Normal,
                    file_type: FileKind::from(PathBuf::from(f.name())),
                },
            )
        })
        .collect::<Vec<(PathBuf, FileMetadata)>>();

    let mut selected = BTreeMap::new();
    for ((p, m), i) in metadata.into_iter().zip(images) {
        selected
            .entry(m.index)
            .and_modify(|((pi, mi), ii)| {
                if m.file_type.is_tiff() {
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

    fs::setup().await?;
    let mut handle = import_files(metadata, &images).await;

    let data;

    if let Some(tse) = other
        .into_iter()
        .find(|f| matches!(FileKind::from(PathBuf::from(&f.name())), FileKind::Tse))
        .cloned()
    {
        let (sender, receiver) = oneshot::channel();
        import_tse(sender, &tse).await?;

        data = receiver.await?;

        // TODO Display error ? Await user input ?
        assert!(data.exposures.len() == images.len())
    } else {
        data = Data::with_count(images.len() as u32)
    }

    controller::set_data(&data)?;

    view::preview::create(images.len() as u32)
        .and(view::landing::hide())
        .and(view::editor::show())?;

    while let Some(import_status) = handle.next().await {
        import_status?;
    }
    generate_thumbnails().await?;

    Ok(())
}

fn set_image(data: JsValue) -> Result<(), Error> {
    let WorkerCompressionAnswer(index, base64): WorkerCompressionAnswer =
        serde_wasm_bindgen::from_value(data)?;

    format!("exposure-{index}-preview")
        .query_id()?
        .set_attribute("src", &format!("data:image/jpeg;base64, {base64}"))?;

    wasm_bindgen_futures::spawn_local(async move {
        controller::notify(controller::Progress::ThumbnailGenerated(index))
            .await
            .aquiesce();
    });

    Ok(())
}

async fn setup_editor_from_data(contents: Data) -> Result<(), Error> {
    fs::setup().await?;

    view::roll::fill_fields(&contents.roll)
        .and(view::preview::create(contents.exposures.len() as u32))
        .and(view::landing::hide())
        .and(view::editor::show())?;

    generate_thumbnails().await
}

async fn generate_thumbnails() -> Result<(), Error> {
    let data = controller::get_metadata()?;
    let mut tasks: Vec<_> = data.into_values().collect();
    tasks.sort_by(|a, b| b.index.cmp(&a.index));

    let tasks: Vec<_> = tasks
        .into_iter()
        .map(WorkerMessage::GenerateThumbnail)
        .collect();

    controller::notify(controller::Progress::ThumbnailStart(tasks.len() as u32)).await?;

    let pool = worker::Pool::try_new_with_callback(tasks, |e: MessageEvent| {
        set_image(e.data()).aquiesce();
    })?;

    pool.join().await?;

    controller::notify(controller::Progress::ThumbnailDone).await?;

    Ok(())
}

#[wasm_bindgen]
pub fn setup() -> JsResult {
    // Listen for keypresses and handle them accordingly
    document()?.on("keydown", &KEY_HANDLER)?;

    view::preview::setup()
        .and(view::landing::setup())
        .and(view::exposure::setup())
        .and(view::roll::setup())?;

    wasm_bindgen_futures::spawn_local(async { controller::handle_progress().await.aquiesce() });

    wasm_bindgen_futures::spawn_local(async { view::landing::landing_stats().await.aquiesce() });

    if let Ok(data) = controller::get_data() {
        wasm_bindgen_futures::spawn_local(async move {
            setup_editor_from_data(data).await.aquiesce();
            controller::update(Update::SelectionClear).aquiesce();
        });
    }

    Ok(())
}
