use crate::{Aquiesce, Error, JsResult, models, process_exposure};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use web_sys::{WorkerOptions, WorkerType};

#[wasm_bindgen]
pub async fn handle_message(data: JsValue) -> Result<(), Error> {
    let message: models::WorkerMessage = serde_wasm_bindgen::from_value(data)?;

    process_exposure(&message.0, &message.1).await
}

#[derive(Clone)]
pub struct Pool {
    expected: usize,
    tasks: Rc<RefCell<Vec<models::WorkerMessage>>>,
    done: Rc<RefCell<usize>>,
    rx: async_channel::Receiver<usize>,
    tx: async_channel::Sender<usize>,
}

impl Pool {
    pub fn new(tasks: Vec<models::WorkerMessage>) -> Self {
        let (tx, rx) = async_channel::bounded(80);

        Self {
            expected: tasks.len(),
            tasks: Rc::new(RefCell::new(tasks)),
            done: Rc::new(RefCell::new(0)),
            rx,
            tx,
        }
    }

    pub fn spawn(self, task: models::WorkerMessage) -> Result<(), Error> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = web_sys::Worker::new_with_options("/worker.js", &options)?;

        let state = self.clone();
        let onmessage = Closure::once(move |_: JsValue| -> JsResult {
            let next = state.tasks.borrow_mut().pop();
            *state.done.borrow_mut() += 1;

            let st = state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                st.tx.send(*st.done.borrow()).await.aquiesce();
            });

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
