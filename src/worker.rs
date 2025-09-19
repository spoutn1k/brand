use crate::{
    Aquiesce, Error, JsError, JsResult, image_management,
    models::{ExposureData, FileMetadata, Orientation},
};
use base64::prelude::*;
use image::{ImageReader, codecs::jpeg::JpegEncoder, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, io::Cursor, path::PathBuf, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::{WorkerOptions, WorkerType};

#[derive(Serialize, Deserialize)]
pub enum WorkerMessage {
    Process(FileMetadata, Box<ExposureData>),
    GenerateThumbnail(FileMetadata),
}

#[derive(Serialize, Deserialize)]
pub struct WorkerCompressionAnswer(pub u32, pub String);

#[derive(Serialize, Deserialize)]
pub struct WorkerProcessingAnswer(pub Vec<PathBuf>);

#[wasm_bindgen]
pub async fn handle_message(data: JsValue) -> JsResult<JsValue> {
    let message: WorkerMessage = serde_wasm_bindgen::from_value(data)?;

    match message {
        WorkerMessage::Process(meta, dat) => process_exposure(&meta, &dat)
            .await
            .and_then(|a| serde_wasm_bindgen::to_value(&a).map_err(|e| e.into())),
        WorkerMessage::GenerateThumbnail(meta) => compress_image(meta)
            .await
            .and_then(|a| serde_wasm_bindgen::to_value(&a).map_err(|e| e.into())),
    }
    .js_error()
}

#[derive(Clone)]
pub struct Pool {
    expected: usize,
    tasks: Rc<RefCell<Vec<WorkerMessage>>>,
    done: Rc<RefCell<usize>>,
    rx: async_channel::Receiver<usize>,
    tx: async_channel::Sender<usize>,
    callback: Rc<Box<dyn Fn(web_sys::MessageEvent)>>,
}

impl Pool {
    pub fn try_new_with_callback(
        tasks: Vec<WorkerMessage>,
        callback: impl Fn(web_sys::MessageEvent) + 'static,
    ) -> Result<Self, Error> {
        let (tx, rx) = async_channel::bounded(80);

        let p = Self {
            expected: tasks.len(),
            tasks: Rc::new(RefCell::new(tasks)),
            done: Rc::new(RefCell::new(0)),
            rx,
            tx,
            callback: Rc::new(Box::new(callback)),
        };

        let concurrency = web_sys::window()
            .ok_or(Error::NoWindow)?
            .navigator()
            .hardware_concurrency() as usize;

        for _ in 1..concurrency {
            p.spawn_next()?;
        }

        Ok(p)
    }

    pub fn try_new(tasks: Vec<WorkerMessage>) -> Result<Self, Error> {
        Self::try_new_with_callback(tasks, Box::new(|_| ()))
    }

    pub fn spawn(self, task: WorkerMessage) -> Result<(), Error> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("/worker.js", &options)?;

        let state = self.clone();
        let onmessage = Closure::once(move |event: web_sys::MessageEvent| -> JsResult {
            let next = state.tasks.borrow_mut().pop();
            *state.done.borrow_mut() += 1;

            let st = state.clone();
            let count = *st.done.borrow();
            wasm_bindgen_futures::spawn_local(async move {
                st.tx.send(count).await.aquiesce();
            });

            state.callback.clone()(event);

            if let Some(task) = next {
                state.spawn(task)?;
            }

            Ok(())
        });

        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        worker.post_message(&serde_wasm_bindgen::to_value(&task)?)?;

        Ok(())
    }

    pub fn spawn_next(&self) -> Result<(), Error> {
        let state = self.clone();
        let task = state.tasks.borrow_mut().pop();

        if let Some(task) = task {
            state.spawn(task)
        } else {
            Ok(())
        }
    }

    pub async fn join(&self) -> Result<(), Error> {
        loop {
            if self.rx.recv().await? == self.expected {
                return Ok(());
            }
        }
    }
}

async fn process_exposure(
    metadata: &FileMetadata,
    data: &ExposureData,
) -> Result<WorkerProcessingAnswer, Error> {
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

    let mut output = Vec::with_capacity(2 * 1024 * 1024); // Reserve 2MB for the output

    image_management::encode_jpeg_with_exif(
        photo
            .clone()
            .resize(2000, 2000, image::imageops::FilterType::Lanczos3),
        Cursor::new(&mut output),
        data,
    )
    .expect("Global error");

    let jpeg = format!("{:0>2}.jpeg", metadata.index);
    web_fs::write(jpeg.clone(), output).await?;

    let mut output = Vec::with_capacity(100 * 1024 * 1024); // Reserve 100MB for the output

    image_management::encode_tiff_with_exif(photo, Cursor::new(&mut output), data)
        .expect("Failed to encode TIFF with EXIF");

    let tiff = format!("{:0>2}.tiff", metadata.index);
    web_fs::write(tiff.clone(), output).await?;

    Ok(WorkerProcessingAnswer(vec![jpeg.into(), tiff.into()]))
}

pub async fn compress_image(meta: FileMetadata) -> Result<WorkerCompressionAnswer, Error> {
    let file = meta.local_fs_path.ok_or(Error::MissingKey(format!(
        "Missing local file for exposure {}",
        meta.index
    )))?;

    let photo = ImageReader::new(Cursor::new(web_fs::read(file).await?))
        .with_guessed_format()?
        .decode()?
        .resize(512, 512, FilterType::Nearest);

    let photo = match meta.orientation {
        Orientation::Normal => photo,
        Orientation::Rotated90 => photo.rotate90(),
        Orientation::Rotated180 => photo.rotate180(),
        Orientation::Rotated270 => photo.rotate270(),
    };

    let mut thumbnail = vec![];
    JpegEncoder::new(&mut thumbnail).encode_image(&photo)?;

    let base64 = BASE64_STANDARD.encode(thumbnail);

    Ok(WorkerCompressionAnswer(meta.index, base64))
}
