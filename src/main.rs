use base64::prelude::*;
use chrono::NaiveDateTime;
use image::io::Reader as ImageReader;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

type JsResult<T = ()> = Result<T, JsValue>;

mod my_date_format {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    pub struct Naive;

    const FORMAT: &'static str = "%Y %m %d %H %M %S";

    impl SerializeAs<NaiveDateTime> for Naive {
        fn serialize_as<S>(value: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let s = format!("{}", value.format(FORMAT));
            serializer.serialize_str(&s)
        }
    }

    impl<'de> DeserializeAs<'de, NaiveDateTime> for Naive {
        fn deserialize_as<D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            Ok(NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?)
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
struct Data {
    roll: RollData,
    exposures: std::collections::HashMap<u32, ExposureSpecificData>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
struct RollData {
    author: Option<String>,
    make: Option<String>,
    model: Option<String>,
    iso: Option<String>,
    description: Option<String>,
}

#[serde_with::serde_as]
#[derive(Clone, Default, Debug, Deserialize, Serialize)]
struct ExposureSpecificData {
    sspeed: Option<String>,
    aperture: Option<String>,
    lens: Option<String>,
    comment: Option<String>,
    #[serde_as(as = "Option<my_date_format::Naive>")]
    date: Option<NaiveDateTime>,
    gps: Option<(f64, f64)>,
}

impl RollData {
    fn update_field(&mut self, key: &str, value: String) {
        match key {
            "author" => self.author = Some(value),
            "make" => self.make = Some(value),
            "model" => self.model = Some(value),
            "iso" => self.iso = Some(value),
            "description" => self.description = Some(value),
            _ => todo!(),
        }
    }

    fn as_tsv(&self) -> String {
        format!(
            "#Description {}
#ImageDescription {}
#Artist {}
#Author {}
#ISO {}
#Make {}
#Model {}
; vim: set list number noexpandtab:",
            self.description.clone().unwrap_or(String::new()),
            self.description.clone().unwrap_or(String::new()),
            self.author.clone().unwrap_or(String::new()),
            self.author.clone().unwrap_or(String::new()),
            self.iso.clone().unwrap_or(String::new()),
            self.make.clone().unwrap_or(String::new()),
            self.model.clone().unwrap_or(String::new()),
        )
    }
}

impl ExposureSpecificData {
    fn update_field(&mut self, key: &str, value: String) {
        match key {
            "sspeed" => self.sspeed = Some(value),
            "aperture" => self.aperture = Some(value),
            "lens" => self.lens = Some(value),
            "comment" => self.comment = Some(value),
            "gps" => {
                self.gps = {
                    let split = value.split(",").collect::<Vec<_>>();
                    if split.len() == 2 {
                        match (
                            split[0].trim().parse::<f64>(),
                            split[1].trim().parse::<f64>(),
                        ) {
                            (Ok(lat), Ok(lon)) => Some((lat, lon)),
                            (Err(e), _) => {
                                log::error!("lat error: {e}");
                                None
                            }
                            (_, Err(e)) => {
                                log::error!("lon error: {e}");
                                None
                            }
                        }
                    } else {
                        log::error!("Invalid gps coordinates format !");
                        None
                    }
                }
            }
            "date" => {
                self.date = NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S")
                    .or(NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M"))
                    .ok()
            }
            _ => todo!(),
        }
    }

    fn as_tsv(&self) -> String {
        let mut fields = vec![
            self.sspeed.clone().unwrap_or(String::new()),
            self.aperture.clone().unwrap_or(String::new()),
            self.lens.clone().unwrap_or(String::new()),
            self.comment.clone().unwrap_or(String::new()),
        ];

        fields.push(
            self.date
                .map(|d| format!("{}", d.format("%Y %m %d %H %M %S")))
                .unwrap_or(String::new()),
        );

        match self.gps {
            None => fields.push(String::new()),
            Some((lat, lon)) => fields.push(format!("{lat}, {lon}")),
        }

        fields.join("\t")
    }
}

impl Data {
    fn to_string(&self) -> String {
        let mut lines: Vec<String> = vec![];
        let max: u32 = *self.exposures.keys().max().unwrap_or(&0u32) + 1;

        for index in 1..max {
            match self.exposures.get(&index) {
                Some(exp) => lines.push(exp.as_tsv()),
                None => lines.push(String::new()),
            }
        }

        lines.push(self.roll.as_tsv());

        lines.join("\n")
    }
}

fn format_image(photo_data: &[u8]) -> JsResult<String> {
    let photo = ImageReader::new(std::io::Cursor::new(photo_data))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;

    let photo = photo.resize(256, 256, image::imageops::FilterType::Nearest);

    let mut jpg = vec![];
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpg);
    encoder.encode_image(&photo).map_err(|e| e.to_string())?;

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
        Err(e) => log::info!("Error: {e:?}"),
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
    setup_general_fields()?;

    let window = web_sys::window().ok_or("No window")?;

    let mut index = std::collections::HashMap::<u32, Vec<web_sys::FileSystemFileEntry>>::new();
    let re = regex::Regex::new(r"([0-9]+)").map_err(|e| e.to_string())?;
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

    let storage = window.session_storage()?.unwrap();
    storage.clear()?;

    let mut template = Data::default();
    for (index, _) in index.iter() {
        template
            .exposures
            .insert(*index, ExposureSpecificData::default());
    }
    storage.set_item("data", &serde_json::to_string(&template).unwrap())?;

    for (index, entries) in index.into_iter() {
        let entry = entries.first().unwrap().to_owned();

        create_row(index, entry)?;
    }

    let document = window.document().ok_or("no document on window")?;

    document
        .get_element_by_id("photoselect")
        .ok_or("No selector !")?
        .class_list()
        .add_1("hidden")?;

    document
        .get_element_by_id("editor")
        .ok_or("No editor !")?
        .class_list()
        .remove_1("hidden")
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

fn roll_field_update_handler(
    event: web_sys::InputEvent,
    field: String,
    storage: web_sys::Storage,
) -> JsResult {
    let content = event
        .target()
        .ok_or("No target for event !")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .value();
    log::info!("Updating {field} with `{content:?}`");

    let mut data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data !")?)
        .map_err(|e| format!("{e}"))?;

    data.roll.update_field(&field, content);

    storage.set_item(
        "data",
        &serde_json::to_string(&data).map_err(|e| format!("{e}"))?,
    )
}

fn exposure_field_update_handler(
    event: web_sys::InputEvent,
    exposure: u32,
    field: String,
    storage: web_sys::Storage,
) -> JsResult {
    let content = event
        .target()
        .ok_or("No target for event !")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .value();
    log::info!("Updating {field} with `{content:?}`");

    let mut data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data !")?)
        .map_err(|e| format!("{e}"))?;

    match data.exposures.get_mut(&exposure) {
        Some(v) => {
            v.update_field(&field, content);

            storage.set_item(
                "data",
                &serde_json::to_string(&data).map_err(|e| format!("{e}"))?,
            )?
        }

        None => log::error!("Failed to access exposure {exposure} !"),
    };

    Ok(())
}

fn set_general_handler(
    field: String,
    input: &web_sys::Element,
    storage: web_sys::Storage,
) -> JsResult {
    let handler = Closure::<dyn Fn(_)>::new(move |i: web_sys::InputEvent| {
        if let Err(e) = roll_field_update_handler(i, field.clone(), storage.clone()) {
            log::error!("{e:?}");
        }
    });

    input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
    handler.forget();

    Ok(())
}

fn set_exposure_handler(
    index: u32,
    field: String,
    input: &web_sys::Element,
    storage: web_sys::Storage,
) -> JsResult {
    let handler = Closure::<dyn Fn(_)>::new(move |i: web_sys::InputEvent| {
        if let Err(e) = exposure_field_update_handler(i, index, field.clone(), storage.clone()) {
            log::error!("{e:?}");
        }
    });

    input.add_event_listener_with_callback("input", handler.as_ref().unchecked_ref())?;
    handler.forget();

    Ok(())
}

fn map_input(index: u32) -> JsResult {
    let storage = web_sys::window()
        .ok_or("No window")?
        .session_storage()?
        .ok_or("No session storage !")?;

    let data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data found !")?)
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
}

fn gen_tse() -> JsResult {
    let storage = web_sys::window()
        .ok_or("No window")?
        .session_storage()?
        .ok_or("No session storage !")?;

    let data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    download_file("index.tse".into(), data.to_string());
    Ok(())
}

fn reset_editor() -> JsResult {
    let window = web_sys::window().ok_or("no global `window` exists")?;
    let document = window.document().ok_or("no document on window")?;

    document
        .get_element_by_id("exposures")
        .ok_or("No exposures !")?
        .set_inner_html("");

    let selector = document
        .get_element_by_id("photoselect")
        .ok_or("No selector !")?;
    selector.class_list().remove_1("hidden")?;
    selector
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value("");

    document
        .get_element_by_id("editor")
        .ok_or("No editor !")?
        .class_list()
        .add_1("hidden")?;

    let storage = window
        .session_storage()?
        .ok_or("No storage for session !")?;

    storage.clear()
}

fn setup_general_fields() -> JsResult {
    let window = web_sys::window().ok_or("no global `window` exists")?;
    let document = window.document().ok_or("no document on window")?;

    let storage = window
        .session_storage()?
        .ok_or("No storage for session !")?;

    let author_input = document
        .get_element_by_id("author-input")
        .ok_or("No author_input !")?;
    author_input.set_attribute("placeholder", "Author")?;
    let make_input = document
        .get_element_by_id("make-input")
        .ok_or("No make_input !")?;
    make_input.set_attribute("placeholder", "Camera Make")?;
    let model_input = document
        .get_element_by_id("model-input")
        .ok_or("No model_input !")?;
    model_input.set_attribute("placeholder", "Camera Model")?;
    let iso_input = document
        .get_element_by_id("iso-input")
        .ok_or("No iso_input !")?;
    iso_input.set_attribute("placeholder", "ISO")?;
    let description_input = document
        .get_element_by_id("description-input")
        .ok_or("No description_input !")?;
    description_input.set_attribute("placeholder", "Film type")?;

    set_general_handler("author".into(), &author_input, storage.clone())?;
    set_general_handler("make".into(), &make_input, storage.clone())?;
    set_general_handler("model".into(), &model_input, storage.clone())?;
    set_general_handler("iso".into(), &iso_input, storage.clone())?;
    set_general_handler("description".into(), &description_input, storage.clone())?;

    let reset = document
        .get_element_by_id("editor-reset")
        .ok_or("No download !")?;

    let reset_editor = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        if let Err(e) = reset_editor() {
            log::error!("{e:?}");
        }
    });
    reset.add_event_listener_with_callback("click", reset_editor.as_ref().unchecked_ref())?;
    reset_editor.forget();

    let download = document
        .get_element_by_id("download")
        .ok_or("No download !")?;

    let download_tse = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        gen_tse().unwrap();
    });

    download.add_event_listener_with_callback("click", download_tse.as_ref().unchecked_ref())?;
    download_tse.forget();

    Ok(())
}

fn update_row_html(index: u32, data: &ExposureSpecificData) -> JsResult {
    let row = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?
        .query_selector(&format!("#exposure-{index}"))?
        .ok_or("No row ?")?;

    let inputs = row.children();
    inputs
        .item(2)
        .ok_or("No input !")?
        .get_elements_by_tag_name("input")
        .item(0)
        .ok_or("Failed")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value(&data.sspeed.clone().unwrap_or(String::new()));
    inputs
        .item(3)
        .ok_or("No input !")?
        .get_elements_by_tag_name("input")
        .item(0)
        .ok_or("Failed")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value(&data.aperture.clone().unwrap_or(String::new()));
    inputs
        .item(4)
        .ok_or("No input !")?
        .get_elements_by_tag_name("input")
        .item(0)
        .ok_or("Failed")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value(&data.lens.clone().unwrap_or(String::new()));
    inputs
        .item(5)
        .ok_or("No input !")?
        .get_elements_by_tag_name("input")
        .item(0)
        .ok_or("Failed")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value(&data.comment.clone().unwrap_or(String::new()));
    inputs
        .item(6)
        .ok_or("No input !")?
        .get_elements_by_tag_name("input")
        .item(0)
        .ok_or("Failed")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value(
            &data
                .date
                .and_then(|d| Some(format!("{}", d.format("%Y-%m-%dT%H:%M:%S"))))
                .unwrap_or(String::new()),
        );
    inputs
        .item(7)
        .ok_or("No input !")?
        .get_elements_by_tag_name("input")
        .item(0)
        .ok_or("Failed")?
        .dyn_into::<web_sys::HtmlInputElement>()?
        .set_value(
            &data
                .gps
                .and_then(|(la, lo)| Some(format!("{la}, {lo}")))
                .unwrap_or(String::new()),
        );

    Ok(())
}

fn clone_row(index: u32, storage: web_sys::Storage) -> JsResult {
    let mut data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    let mut exposures: Vec<(u32, ExposureSpecificData)> = data.exposures.into_iter().collect();
    exposures.sort_by_key(|e| e.0);

    let current = exposures.iter_mut().position(|k| k.0 == index);

    if let Some(position) = current {
        if position + 1 < exposures.len() {
            let (_, data) = exposures[position].clone();
            let (target, _) = exposures[position + 1].clone();
            exposures[position + 1] = (target, data.clone());
            update_row_html(target, &data)?;
        }
    }

    data.exposures = exposures.into_iter().collect();

    storage.set_item(
        "data",
        &serde_json::to_string(&data).map_err(|e| e.to_string())?,
    )?;

    Ok(())
}

fn create_row(index: u32, photo: web_sys::FileSystemFileEntry) -> JsResult {
    let window = web_sys::window().ok_or("no global `window` exists")?;
    let document = window.document().ok_or("no document on window")?;

    let table = document
        .query_selector("table#exposures")?
        .ok_or("No table ?")?;

    let storage = window
        .session_storage()?
        .ok_or("No storage for session !")?;

    let row = el!(document, "tr");
    row.set_id(&format!("exposure-{index}"));
    let id = el!(document, "td");
    let icon = el!(document, "td");
    let image = el!(document, "img");

    let sspeed = el!(document, "td");
    let aperture = el!(document, "td");
    let lens = el!(document, "td");
    let comment = el!(document, "td");
    let date = el!(document, "td");
    let gps = el!(document, "td");
    let options = el!(document, "td");

    let sspeed_input = el!(document, "input");
    sspeed_input.set_id(&format!("sspeed-input-{index}"));
    sspeed_input.set_attribute("placeholder", "Shutter Speed")?;
    let aperture_input = el!(document, "input");
    aperture_input.set_attribute("placeholder", "Aperture")?;
    let lens_input = el!(document, "input");
    lens_input.set_attribute("placeholder", "Focal length")?;
    let comment_input = el!(document, "input");
    comment_input.set_attribute("placeholder", "Title")?;
    let date_input = el!(document, "input");
    date_input.set_attribute("type", "datetime-local")?;
    date_input.set_attribute("step", "1")?;
    let gps_input = el!(document, "input");
    gps_input.set_id(&format!("gps-input-{index}"));
    gps_input.set_attribute("placeholder", "GPS coordinates")?;
    let gps_select = el!(document, "input").dyn_into::<web_sys::HtmlInputElement>()?;
    gps_select.set_attribute("type", "button")?;
    gps_select.set_value("Map");
    let clone_down = el!(document, "input").dyn_into::<web_sys::HtmlInputElement>()?;
    clone_down.set_attribute("type", "button")?;
    clone_down.set_value("Clone below");

    set_exposure_handler(index, "sspeed".into(), &sspeed_input, storage.clone())?;
    set_exposure_handler(index, "aperture".into(), &aperture_input, storage.clone())?;
    set_exposure_handler(index, "lens".into(), &lens_input, storage.clone())?;
    set_exposure_handler(index, "comment".into(), &comment_input, storage.clone())?;
    set_exposure_handler(index, "date".into(), &date_input, storage.clone())?;
    set_exposure_handler(index, "gps".into(), &gps_input, storage.clone())?;

    let coords_select = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        if let Err(e) = map_input(index) {
            log::error!("{e:?}");
        }
    });
    gps_select.add_event_listener_with_callback("click", coords_select.as_ref().unchecked_ref())?;
    coords_select.forget();

    let clone_down_action = Closure::<dyn Fn(_)>::new(move |_: web_sys::Event| {
        if let Err(e) = clone_row(index, storage.clone()) {
            log::error!("{e:?}");
        }
    });
    clone_down
        .add_event_listener_with_callback("click", clone_down_action.as_ref().unchecked_ref())?;
    clone_down_action.forget();

    sspeed.append_with_node_1(&sspeed_input)?;
    aperture.append_with_node_1(&aperture_input)?;
    lens.append_with_node_1(&lens_input)?;
    comment.append_with_node_1(&comment_input)?;
    date.append_with_node_1(&date_input)?;
    gps.append_with_node_2(&gps_input, &gps_select)?;
    options.append_with_node_1(&clone_down)?;

    let img_id = hash(photo.name());
    image.set_id(&img_id);
    image.set_attribute("alt", &format!("Exposure n{}", index))?;
    let file_load = Closure::<dyn Fn(_)>::new(move |f: web_sys::File| {
        read_file(f, img_id.clone()).unwrap();
    });

    photo.file_with_callback(file_load.as_ref().unchecked_ref());
    file_load.forget();

    id.set_text_content(Some(&format!("{index}")));
    icon.append_with_node_1(&image)?;

    row.append_with_node_7(&id, &icon, &sspeed, &aperture, &lens, &comment, &date)?;
    row.append_with_node_2(&gps, &options)?;
    table.append_with_node_1(&row)
}

#[wasm_bindgen]
pub fn update_coords(index: u32, lat: f64, lon: f64) {
    log::debug!("Updating coords for exposure {index}: {lat} / {lon}!");

    if let Err(e) = update_coords_error(index, lat, lon) {
        log::error!("{e:?}");
    }
}

fn update_coords_error(index: u32, lat: f64, lon: f64) -> JsResult {
    let storage = web_sys::window()
        .ok_or("No window")?
        .session_storage()?
        .ok_or("No storage !")?;

    let mut data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    data.exposures
        .get_mut(&index)
        .ok_or("Failed to access exposure")?
        .gps = Some((lat, lon));

    storage.set_item(
        "data",
        &serde_json::to_string(&data).map_err(|e| e.to_string())?,
    )?;

    let document = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?;

    let container = document
        .query_selector(&format!("#gps-input-{index}"))?
        .ok_or("no selector !")?
        .dyn_into::<web_sys::HtmlInputElement>()?;

    container.set_value(&format!("{lat}, {lon}"));

    Ok(())
}

fn wrapper_webkit() -> JsResult {
    let selector = web_sys::window()
        .ok_or("no global `window` exists")?
        .document()
        .ok_or("no document on window")?
        .query_selector("#photoselect")?
        .ok_or("no selector !")?
        .dyn_into::<web_sys::HtmlInputElement>()?;

    disable_click(&selector)?;

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

fn disable_click(selector: &web_sys::HtmlInputElement) -> JsResult {
    let disable_click =
        Closure::<dyn Fn(_)>::new(move |e: web_sys::InputEvent| e.prevent_default());
    selector.add_event_listener_with_callback("click", disable_click.as_ref().unchecked_ref())?;
    disable_click.forget();

    Ok(())
}

#[wasm_bindgen]
extern "C" {
    fn set_marker(x: f64, y: f64);
    fn prompt_coords(i: u32);
    fn download_file(filename: String, contents: String);
}

fn main() {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    if let Err(e) = wrapper_webkit() {
        log::error!("{}", e.as_string().unwrap())
    }
}
