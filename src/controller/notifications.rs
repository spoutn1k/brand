use crate::{Aquiesce, Error, JsError, JsResult, QueryExt, helpers::window};
use async_channel::{Receiver, Sender};
use std::cell::Cell;
use wasm_bindgen::{JsCast, closure::Closure};

thread_local! {
static CHANNEL: (Sender<Progress>, Receiver<Progress>) = async_channel::bounded(80);

static TIMEOUT: Cell<i32> = Cell::new(0);
static HIDE_NOTIFICATIONS: Closure<dyn Fn() -> JsResult> = Closure::new(move || -> JsResult {
    "notifications"
        .query_id()
        .and_then(|n| n.class_list().add_1("hidden").map_err(|e| e.into()))
        .js_error()
});

static THUMBNAIL_TRACKER: Cell<(u32, u32)> = Cell::new((0, 0));
static THUMBNAIL_TIMEOUT: Cell<i32> = Cell::new(0);
static HIDE_THUMBNAILS: Closure<dyn Fn() -> JsResult> = Closure::new(move || -> JsResult {
    "thumbnails"
        .query_id()
        .and_then(|n| n.class_list().add_1("hidden").map_err(|e| e.into()))
        .js_error()
});

static PROCESSING_TRACKER: Cell<(u32, u32)> = Cell::new((0, 0));
static PROCESSING_TIMEOUT: Cell<i32> = Cell::new(0);
static HIDE_PROCESSING: Closure<dyn Fn() -> JsResult> = Closure::new(move || -> JsResult {
    "processing"
        .query_id()
        .and_then(|n| n.class_list().add_1("hidden").map_err(|e| e.into()))
        .js_error()
});
}

#[derive(Debug)]
pub enum Progress {
    ProcessingStart(u32),
    Processing(u32),
    ProcessingDone,
    ThumbnailGenerated(u32),
    ThumbnailStart(u32),
    ThumbnailDone,
}

pub fn notifier() -> Sender<Progress> {
    CHANNEL.with(|t| t.0.clone())
}

pub fn sender() -> Receiver<Progress> {
    CHANNEL.with(|t| t.1.clone())
}

pub async fn handle_progress() -> Result<(), Error> {
    while let Ok(data) = sender().recv().await {
        match data {
            Progress::ThumbnailStart(count) => THUMBNAIL_TRACKER.set((0, count)),
            Progress::ThumbnailGenerated(_) => {
                let (done, count) = THUMBNAIL_TRACKER.get();
                THUMBNAIL_TRACKER.set((done + 1, count));
            }
            Progress::ThumbnailDone => THUMBNAIL_TRACKER.set((0, 0)),
            Progress::ProcessingStart(count) => PROCESSING_TRACKER.set((0, count)),
            Progress::Processing(_) => {
                let (done, count) = PROCESSING_TRACKER.get();
                PROCESSING_TRACKER.set((done + 1, count));
            }
            Progress::ProcessingDone => PROCESSING_TRACKER.set((0, 0)),
        }

        display_progress().aquiesce();
    }

    Ok(())
}

fn display_progress() -> Result<(), Error> {
    let mut in_progress = false;

    let thumbnails = "thumbnails".query_id()?;
    let (done, count) = THUMBNAIL_TRACKER.get();
    if count > 0 {
        in_progress |= true;

        let handle = THUMBNAIL_TIMEOUT.get();
        if handle > 0 {
            window()?.clear_timeout_with_handle(handle);
        }

        thumbnails.class_list().remove_1("hidden")?;
        thumbnails.set_text_content(Some(&format!("Generating thumbnails ({done}/{count})")));
    } else {
        let handle = HIDE_THUMBNAILS.try_with(|handler| {
            window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                handler.as_ref().unchecked_ref(),
                1000,
            )
        })??;
        THUMBNAIL_TIMEOUT.set(handle);

        thumbnails.set_text_content(Some(&format!("Generating thumbnails done.")));
    }

    let processing = "processing".query_id()?;
    let (done, count) = PROCESSING_TRACKER.get();
    if count > 0 {
        in_progress |= true;

        let handle = PROCESSING_TIMEOUT.get();
        if handle > 0 {
            window()?.clear_timeout_with_handle(handle);
        }

        processing.class_list().remove_1("hidden")?;
        processing.set_text_content(Some(&format!("Processing ({done}/{count})")));
    } else {
        let handle = HIDE_PROCESSING.try_with(|handler| {
            window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                handler.as_ref().unchecked_ref(),
                1000,
            )
        })??;
        PROCESSING_TIMEOUT.set(handle);

        processing.set_text_content(Some(&format!("Processing done.")));
    }

    if in_progress {
        let handle = TIMEOUT.get();
        if handle > 0 {
            window()?.clear_timeout_with_handle(handle);
        }
        "notifications"
            .query_id()?
            .class_list()
            .remove_1("hidden")?;
    } else {
        let handle = HIDE_NOTIFICATIONS.try_with(|handler| {
            window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                handler.as_ref().unchecked_ref(),
                1000,
            )
        })??;
        TIMEOUT.set(handle);
    }

    Ok(())
}
