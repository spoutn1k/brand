use base64::prelude::*;
use chrono::NaiveDateTime;
use image::io::Reader as ImageReader;
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

type JsResult<T = ()> = Result<T, JsValue>;

#[derive(Clone, Default, Debug)]
struct ExposureData {
    author: Option<String>,
    make: Option<String>,
    model: Option<String>,
    sspeed: Option<String>,
    aperture: Option<String>,
    iso: Option<String>,
    lens: Option<String>,
    description: Option<String>,
    comment: Option<String>,
    date: Option<NaiveDateTime>,
    gps: Option<(f64, f64)>,
}

fn format_image(photo_data: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let photo = ImageReader::new(std::io::Cursor::new(photo_data))
        .with_guessed_format()?
        .decode()?;

    let photo = photo.resize(256, 256, image::imageops::FilterType::Nearest);

    let mut jpg = vec![];
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpg);
    encoder.encode_image(&photo)?;

    log::debug!("Size: {} -> {}", photo_data.len(), jpg.len());

    Ok(BASE64_STANDARD.encode(jpg))
}

fn embed_file(photo_data: &[u8], target: String) -> JsResult {
    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let image_element = document
        .query_selector(&format!("img#{target}"))?
        .ok_or("no image target !")?;

    match format_image(photo_data) {
        Ok(formatted) => image_element.set_attribute(
            "src",
            &format!("data:image/{};base64, {}", "jpeg", formatted),
        )?,
        Err(e) => log::info!("Error: {e}"),
    }

    Ok(())
}

fn read_file(file: web_sys::File, target: String) -> JsResult {
    let reader = web_sys::FileReader::new()?;
    reader.read_as_array_buffer(&file)?;

    let r = reader.clone();
    let closure = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        log::debug!("Done loading file");
        match r.result() {
            Ok(buffer) => {
                let data = js_sys::Uint8Array::new(&buffer);

                // Create a Rust slice from the Uint8Array
                let t = target.clone();
                if let Err(e) = embed_file(&data.to_vec(), t) {
                    log::error!("Error embedding file: {e:?}");
                }
            }

            Err(e) => log::error!("Failed to access result: {}", e.as_string().unwrap()),
        }
    });

    reader.set_onloadend(Some(&closure.as_ref().unchecked_ref()));
    closure.forget();

    let error_handler = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        log::error!("Failed to read file !");
    });

    reader.set_onerror(Some(&error_handler.as_ref().unchecked_ref()));
    error_handler.forget();

    Ok(())
}

fn setup_editor(files: &Vec<web_sys::FileSystemFileEntry>) -> JsResult {
    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let mut index = std::collections::HashMap::<u32, Vec<web_sys::FileSystemFileEntry>>::new();
    let re = regex::Regex::new(r"([0-9]+)").unwrap();
    let _ = files
        .into_iter()
        .map(|f| (String::from(f.name()), f.to_owned()))
        .filter_map(|(name, file): (String, web_sys::FileSystemFileEntry)| {
            match re.captures(&name) {
                Some(value) => {
                    let (index, _) = value.extract::<1>();
                    Some((String::from(index), file))
                }
                None => None,
            }
        })
        .for_each(|(index_str, file)| {
            str::parse::<u32>(&index_str)
                .and_then(|i| match index.get_mut(&i) {
                    Some(container) => Ok(container.push(file)),
                    None => {
                        index.insert(i, vec![file]);
                        Ok(())
                    }
                })
                .ok();
        });

    let mut index: Vec<(u32, Vec<web_sys::FileSystemFileEntry>)> = index.into_iter().collect();
    index.sort_by(|lhs, rhs| lhs.0.partial_cmp(&rhs.0).unwrap());

    log::info!("Files: {index:#?}");

    for (index, entries) in index.into_iter() {
        let entry = entries.first().unwrap().to_owned();

        create_row(index, entry)?;
    }

    Ok(())
}

macro_rules! el {
    ($src:expr, $type:expr) => {
        $src.create_element($type)
            .expect("Failed to create element !")
    };
}

fn hash(input: String) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);

    format!(
        "i{}",
        hasher
            .finalize()
            .iter()
            .map(|u| format!("{u:x}"))
            .collect::<Vec<_>>()
            .join("")
    )
}

fn create_row(index: u32, photo: web_sys::FileSystemFileEntry) -> JsResult {
    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let table = document
        .query_selector("table#editor")?
        .ok_or("No table ?")?;

    let row = el!(document, "tr");
    let id = el!(document, "td");
    let icon = el!(document, "td");
    let image = el!(document, "img");

    let sspeed = el!(document, "td");
    let aperture = el!(document, "td");
    let lens = el!(document, "td");
    let description = el!(document, "td");
    let date = el!(document, "td");
    let gps = el!(document, "td");

    let sspeed_input = el!(document, "input");
    let aperture_input = el!(document, "input");
    let lens_input = el!(document, "input");
    let description_input = el!(document, "input");
    let date_input = el!(document, "input");
    let gps_input = el!(document, "input");

    sspeed.append_with_node_1(&sspeed_input)?;
    aperture.append_with_node_1(&aperture_input)?;
    lens.append_with_node_1(&lens_input)?;
    description.append_with_node_1(&description_input)?;

    let img_id = hash(photo.name());
    image.set_id(&img_id);
    let file_load = Closure::<dyn Fn(_)>::new(move |f: web_sys::File| {
        read_file(f, img_id.clone()).unwrap();
    });

    photo.file_with_callback(file_load.as_ref().unchecked_ref());
    file_load.forget();

    id.set_text_content(Some(&format!("{index}")));
    icon.append_with_node_1(&image)?;

    row.append_with_node_7(&id, &icon, &sspeed, &aperture, &lens, &description, &date)?;
    row.append_with_node_1(&gps)?;
    table.append_with_node_1(&row)
}

fn wrapper_webkit() -> JsResult {
    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let selector = document
        .query_selector("#photoselect")?
        .ok_or("no selector !")?
        .dyn_into::<web_sys::HtmlInputElement>()?;

    let target = selector.clone();
    let closure = Closure::<dyn Fn(_)>::new(move |_: web_sys::InputEvent| {
        let files = target
            .webkit_entries()
            .iter()
            .map(|f| f.dyn_into::<web_sys::FileSystemFileEntry>().unwrap())
            .collect();

        setup_editor(&files).unwrap();
    });

    selector.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());

    if let Err(e) = wrapper_webkit() {
        log::error!("{}", e.as_string().unwrap())
    }
}
