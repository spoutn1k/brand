use crate::{Aquiesce, Error, JsError, JsResult, QueryExt, helpers::window};
use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{Sender, channel},
};
use std::cell::{OnceCell, RefCell};
use wasm_bindgen::{JsCast, closure::Closure};

thread_local! {
static CHANNEL: OnceCell<Sender<Progress>> = OnceCell::new();

static MANAGER: RefCell<Manager> = RefCell::new(Default::default());

static HIDE_NOTIFICATIONS: Closure<dyn Fn() -> JsResult> = hide("notifications");
static HIDE_THUMBNAILS: Closure<dyn Fn() -> JsResult> = hide("thumbnails");
static HIDE_PROCESSING: Closure<dyn Fn() -> JsResult> = hide("processing");
}

fn hide(id: &'static str) -> Closure<dyn Fn() -> JsResult> {
    Closure::new(move || -> JsResult {
        id.query_id()
            .js_error()
            .and_then(|n| n.class_list().add_1("hidden"))
    })
}

#[derive(Default)]
struct CompletionTracker {
    complete: u32,
    expected: u32,
    timeout_handle: i32,
}

impl CompletionTracker {
    fn new(expected: u32) -> Self {
        Self {
            expected,
            ..Default::default()
        }
    }

    fn inc(&mut self) {
        self.complete += 1;
    }
}

#[derive(Default)]
struct Manager {
    global_timeout_handle: i32,

    thumbnails: CompletionTracker,
    processing: CompletionTracker,
}

impl Manager {
    fn process(&mut self, update: Progress) {
        match update {
            Progress::ThumbnailStart(count) => self.thumbnails = CompletionTracker::new(count),
            Progress::ThumbnailGenerated(_) => self.thumbnails.inc(),
            Progress::ThumbnailDone => self.thumbnails = Default::default(),
            Progress::ProcessingStart(count) => self.processing = CompletionTracker::new(count),
            Progress::Processing(_) => self.processing.inc(),
            Progress::ProcessingDone => self.processing = Default::default(),
        }

        self.display_progress().aquiesce();
    }

    fn display_progress(&mut self) -> Result<(), Error> {
        let mut in_progress = false;

        let thumbnails = "thumbnails".query_id()?;
        if self.thumbnails.expected > 0 {
            in_progress |= true;

            if self.thumbnails.timeout_handle > 0 {
                window()?.clear_timeout_with_handle(self.thumbnails.timeout_handle);
            }

            thumbnails.class_list().remove_1("hidden")?;
            thumbnails.set_text_content(Some(&format!(
                "Generating thumbnails ({}/{})",
                self.thumbnails.complete, self.thumbnails.expected
            )));
        } else {
            self.thumbnails.timeout_handle = HIDE_THUMBNAILS.try_with(|handler| {
                window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                    handler.as_ref().unchecked_ref(),
                    2000,
                )
            })??;

            thumbnails.set_text_content(Some(&format!("Generating thumbnails done.")));
        }

        let processing = "processing".query_id()?;
        if self.processing.expected > 0 {
            in_progress |= true;

            if self.processing.timeout_handle > 0 {
                window()?.clear_timeout_with_handle(self.processing.timeout_handle);
            }

            processing.class_list().remove_1("hidden")?;
            processing.set_text_content(Some(&format!(
                "Processing ({}/{})",
                self.processing.complete, self.processing.expected
            )));
        } else {
            self.processing.timeout_handle = HIDE_PROCESSING.try_with(|handler| {
                window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                    handler.as_ref().unchecked_ref(),
                    2000,
                )
            })??;

            processing.set_text_content(Some(&format!("Processing done.")));
        }

        if in_progress {
            if self.global_timeout_handle > 0 {
                window()?.clear_timeout_with_handle(self.global_timeout_handle);
            }
            "notifications"
                .query_id()?
                .class_list()
                .remove_1("hidden")?;
        } else {
            self.global_timeout_handle = HIDE_NOTIFICATIONS.try_with(|handler| {
                window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                    handler.as_ref().unchecked_ref(),
                    2000,
                )
            })??;
        }

        Ok(())
    }
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

pub async fn handle_progress() -> Result<(), Error> {
    let (sender, mut receiver) = channel(80);

    CHANNEL
        .with(move |oc| oc.set(sender))
        .map_err(|_| Error::MissingKey(String::from("Failed to set sender in thread storage")))?;

    while let Some(data) = receiver.next().await {
        MANAGER.with_borrow_mut(|manager| manager.process(data))
    }

    Ok(())
}

pub async fn notify(update: Progress) -> Result<(), Error> {
    if let Some(mut notifier) = CHANNEL.with(|t| t.get().cloned()) {
        notifier.send(update).await?;
    }

    Ok(())
}
