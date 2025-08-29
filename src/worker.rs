use crate::{Aquiesce, Error, JsError, JsResult, models, process_exposure};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::{WorkerOptions, WorkerType};

#[wasm_bindgen]
pub async fn handle_message(data: JsValue) -> JsResult<JsValue> {
    let message: models::WorkerMessage = serde_wasm_bindgen::from_value(data)?;

    match message {
        models::WorkerMessage::Process(meta, dat) => process_exposure(&meta, &dat)
            .await
            .map(|_| JsValue::null())
            .js_error(),
        models::WorkerMessage::GenerateThumbnail(meta) => crate::controller::compress_image(meta)
            .await
            .and_then(|a| serde_wasm_bindgen::to_value(&a).map_err(|e| e.into()))
            .js_error(),
    }
}

#[derive(Clone)]
pub struct Pool {
    expected: usize,
    tasks: Rc<RefCell<Vec<models::WorkerMessage>>>,
    done: Rc<RefCell<usize>>,
    rx: async_channel::Receiver<usize>,
    tx: async_channel::Sender<usize>,
    callback: Rc<Box<dyn Fn(web_sys::MessageEvent)>>,
}

impl Pool {
    pub fn try_new_with_callback(
        tasks: Vec<models::WorkerMessage>,
        callback: Box<dyn Fn(web_sys::MessageEvent)>,
    ) -> Result<Self, Error> {
        let (tx, rx) = async_channel::bounded(80);

        let p = Self {
            expected: tasks.len(),
            tasks: Rc::new(RefCell::new(tasks)),
            done: Rc::new(RefCell::new(0)),
            rx,
            tx,
            callback: Rc::new(callback),
        };

        let concurrency = web_sys::window()
            .ok_or(Error::Macro(crate::MacroError::NoWindow))?
            .navigator()
            .hardware_concurrency() as usize;

        for _ in 1..concurrency {
            p.spawn_next()?;
        }

        Ok(p)
    }

    pub fn try_new(tasks: Vec<models::WorkerMessage>) -> Result<Self, Error> {
        Self::try_new_with_callback(tasks, Box::new(|_| ()))
    }

    pub fn spawn(self, task: models::WorkerMessage) -> Result<(), Error> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("/worker.js", &options)?;

        let state = self.clone();
        let onmessage = Closure::once(move |event: web_sys::MessageEvent| -> JsResult {
            let next = state.tasks.borrow_mut().pop();
            *state.done.borrow_mut() += 1;

            let st = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                st.tx.send(*st.done.borrow()).await.aquiesce();
            });

            state.callback.clone()(event);

            if let Some(task) = next {
                state.spawn(task)?;
            }

            Ok(())
        });

        worker.set_onmessage(Some(&onmessage.as_ref().unchecked_ref()));
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
