use wasm_bindgen::prelude::*;

type JsResult<T = ()> = Result<T, JsValue>;

fn embed_file(file: web_sys::File, target: String) -> JsResult {
    log::info!("Embedding file {} into {target}", file.name());

    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    /*
    let image = document
        .query_selector(&format!("img#{target}"))
        .unwrap()
        .ok_or("no image target !")?;
    */

    let reader = web_sys::FileReader::new()?;
    reader.read_as_binary_string(&file)?;

    let r = reader.clone();
    let closure = Closure::<dyn Fn(_)>::new(move |_: web_sys::LoadEvent| {
        log::info!("Read: {}", r.result().unwrap().as_string().unwrap());
    });

    reader.set_onloadend(Some(&closure.into()));
    closure.forget();

    Ok(())
}

fn wrapper_webkit() -> JsResult {
    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let selector = document
        .query_selector("#photoselect")
        .unwrap()
        .ok_or("no selector !")?
        .dyn_into::<web_sys::HtmlInputElement>()?;

    let target = selector.clone();
    let closure = Closure::<dyn Fn(_)>::new(move |_: web_sys::InputEvent| {
        let files = target.webkit_entries();
        log::info!("{} files selected !", files.length());
        files.iter().for_each(|f| {
            let entry = f.dyn_into::<web_sys::FileSystemFileEntry>().unwrap();
            log::info!(
                "File {} - {}",
                entry.name(),
                entry.filesystem().root().full_path()
            );

            let image = document
                .create_element("img")
                .expect("Failed to create element !");

            let id = entry.name().replace('.', "_");

            image.set_id(&id);

            document
                .body()
                .unwrap()
                .append_with_node_1(&image)
                .expect("Failed to add node !");

            let closure = Closure::<dyn Fn(_)>::new(move |f: web_sys::File| {
                let target = id.clone();
                embed_file(f, target).unwrap();
            });

            entry.file_with_callback(closure.as_ref().unchecked_ref());
            closure.forget();
        });
    });
    selector.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}

/*
fn wrapper() -> JsResult {
    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let selector = document
        .query_selector("#photoselect")
        .unwrap()
        .ok_or("no selector !")?
        .dyn_into::<web_sys::HtmlInputElement>()?;

    let target = selector.clone();
    let closure = Closure::<dyn Fn(_)>::new(move |_: web_sys::InputEvent| {
        let files = target.files().unwrap();
        let files = (0..files.length())
            .map(|i| files.item(i).unwrap())
            .collect::<Vec<_>>();
        log::info!("{} files selected !", files.len());
        files.iter().for_each(|f| {
            log::info!("File {}", f.name());

            let image = document
                .create_element("img")
                .expect("Failed to create element !");
            image
                .set_attribute("src", &f.name())
                .expect("Could not set attribute !");

            document
                .body()
                .unwrap()
                .append_with_node_1(&image)
                .expect("Failed to add node !");
        });
    });
    selector.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}
*/

fn main() {
    wasm_logger::init(wasm_logger::Config::default());

    if let Err(e) = wrapper_webkit() {
        log::error!("{}", e.as_string().unwrap())
    }
}
