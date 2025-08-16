use crate::{
    JsResult,
    macros::{MacroError, el, query_id, storage},
    models::{Data, ExposureSpecificData, MAX_EXPOSURES, Selection},
};
use base64::prelude::*;
use chrono::NaiveDateTime;
use image::{ImageFormat, ImageReader, codecs::jpeg::JpegEncoder, imageops::FilterType};
use std::{collections::HashMap, convert::TryInto, io::Cursor};
use wasm_bindgen::{JsCast, JsValue};

pub enum Update {
    ExposureImage(u32, String),
    ExposureImageRestore(u32),
    SelectExposure(u32, bool, bool),
    SelectionClear,
    SelectionAll,
    SelectionInvert,
    Exposure(u32, ExposureSpecificData),
    ExposureField(u32, UIExposureUpdate),
    Roll(UIRollUpdate),
}

#[derive(Debug, Clone)]
pub enum UIRollUpdate {
    Author(String),
    Make(String),
    Model(String),
    Iso(String),
    Film(String),
}

#[derive(Debug, Clone)]
pub enum RollUpdate {
    Author(Option<String>),
    Make(Option<String>),
    Model(Option<String>),
    Iso(Option<String>),
    Film(Option<String>),
}

#[derive(Debug, Clone)]
pub enum UIExposureUpdate {
    ShutterSpeed(String),
    Aperture(String),
    Lens(String),
    Comment(String),
    Date(String),
    Gps(String),
}

#[derive(Debug, Clone)]
pub enum ExposureUpdate {
    ShutterSpeed(Option<String>),
    Aperture(Option<String>),
    Lens(Option<String>),
    Comment(Option<String>),
    Date(Option<NaiveDateTime>),
    Gps(Option<(f64, f64)>),
}

impl TryInto<ExposureUpdate> for UIExposureUpdate {
    type Error = JsValue;

    fn try_into(self) -> Result<ExposureUpdate, Self::Error> {
        Ok(match self {
            UIExposureUpdate::ShutterSpeed(s) => {
                ExposureUpdate::ShutterSpeed(if !s.is_empty() { Some(s) } else { None })
            }
            UIExposureUpdate::Aperture(s) => {
                ExposureUpdate::Aperture(if !s.is_empty() { Some(s) } else { None })
            }
            UIExposureUpdate::Comment(s) => {
                ExposureUpdate::Comment(if !s.is_empty() { Some(s) } else { None })
            }
            UIExposureUpdate::Lens(s) => {
                ExposureUpdate::Lens(if !s.is_empty() { Some(s) } else { None })
            }
            UIExposureUpdate::Date(value) => ExposureUpdate::Date(Some(
                NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S")
                    .or(NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M"))
                    .map_err(|e| e.to_string())?,
            )),
            UIExposureUpdate::Gps(value) => {
                let split = value.split(",").collect::<Vec<_>>();
                if split.len() == 2 {
                    match (
                        split[0].trim().parse::<f64>(),
                        split[1].trim().parse::<f64>(),
                    ) {
                        (Ok(lat), Ok(lon)) => ExposureUpdate::Gps(Some((lat, lon))),
                        (Err(_), _) => {
                            Err(format!("Unrecognised format for latitude: {}", split[0]))?
                        }
                        (_, Err(_)) => {
                            Err(format!("Unrecognised format for longitude: {}", split[1]))?
                        }
                    }
                } else {
                    Err("Invalid gps coordinates format !")?
                }
            }
        })
    }
}

impl TryInto<RollUpdate> for UIRollUpdate {
    type Error = wasm_bindgen::prelude::JsValue;

    fn try_into(self) -> Result<RollUpdate, Self::Error> {
        Ok(match self {
            UIRollUpdate::Author(s) => {
                RollUpdate::Author(if !s.is_empty() { Some(s) } else { None })
            }
            UIRollUpdate::Make(s) => RollUpdate::Make(if !s.is_empty() { Some(s) } else { None }),
            UIRollUpdate::Model(s) => RollUpdate::Model(if !s.is_empty() { Some(s) } else { None }),
            UIRollUpdate::Iso(s) => RollUpdate::Iso(if !s.is_empty() { Some(s) } else { None }),
            UIRollUpdate::Film(s) => RollUpdate::Film(if !s.is_empty() { Some(s) } else { None }),
        })
    }
}

fn exposure_update_field(index: u32, change: UIExposureUpdate) -> JsResult {
    let validated: ExposureUpdate = change.clone().try_into()?;

    let storage = storage!();

    let mut data: Data =
        serde_json::from_str(&storage.get_item("data")?.ok_or("No data in storage !")?)
            .map_err(|e| format!("{e}"))?;

    let selection: Selection = get_selection()?;

    for target in std::iter::once(index).chain(selection.items()) {
        data.exposures
            .get_mut(&target)
            .ok_or("Failed to access exposure")?
            .update(validated.clone());

        if target != index {
            update_exposure_ui(target, &change)?;
        }
    }

    update_exposure_ui(index, &change)?;

    storage.set_item(
        "data",
        &serde_json::to_string(&data).map_err(|e| format!("{e}"))?,
    )
}

macro_rules! image_cache {
    () => {
        serde_json::from_str::<HashMap<u32, String>>(
            &storage!().get_item("image_cache")?.unwrap_or("{}".into()),
        )
        .map_err(|e| format!("{e}"))?
    };
}

fn set_exposure_selection(index: u32, selected: bool) -> JsResult {
    query_id!(
        &format!("exposure-input-select-{index}"),
        web_sys::HtmlInputElement
    )
    .set_checked(selected);

    let classes = query_id!(&format!("exposure-{index}")).class_list();
    if selected {
        preview_exposure(index)?;

        classes.add_1("selected")
    } else {
        preview_exposure_cancel(index)?;

        classes.remove_1("selected")
    }
}

fn preview_exposure(index: u32) -> JsResult {
    query_id!(&format!("exposure-{index}-preview")).remove_attribute("hidden");
    Ok(())
}

fn preview_exposure_cancel(index: u32) -> JsResult {
    query_id!(&format!("exposure-{index}-preview")).set_attribute("hidden", "true");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error(transparent)]
    Macro(#[from] MacroError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("JS error: {0}")]
    Js(String),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

impl From<JsValue> for ControllerError {
    fn from(value: JsValue) -> Self {
        ControllerError::Js(
            value
                .as_string()
                .unwrap_or_else(|| "Unknown JS error".to_string()),
        )
    }
}

impl From<ControllerError> for JsValue {
    fn from(value: ControllerError) -> Self {
        JsValue::from_str(&value.to_string())
    }
}

async fn exposure_update_image(index: u32, filename: String) -> Result<(), ControllerError> {
    log::info!("Updating image for exposure {index} with file {filename}");
    //let mut image_cache = image_cache!();

    let photo = ImageReader::new(Cursor::new(web_fs::read(&filename).await?))
        .with_guessed_format()?
        .decode()?;

    let mut thumbnail = vec![];
    JpegEncoder::new(&mut thumbnail).encode_image(&photo.resize(512, 512, FilterType::Nearest))?;

    let base64 = BASE64_STANDARD.encode(thumbnail);

    query_id!(&format!("exposure-{index}-preview"))
        .set_attribute("src", &format!("data:image/jpeg;base64, {base64}"));

    Ok(())
}

fn exposure_restore_image(index: u32) -> JsResult {
    let image_cache = image_cache!();

    let data = image_cache
        .get(&index)
        .ok_or(format!("No image cached for exposure {index}"))?;

    query_id!(&format!("exposure-{index}-preview"))
        .set_attribute("src", &format!("data:image/jpeg;base64, {data}"))
}

fn roll_update(change: UIRollUpdate) -> JsResult {
    let validated: RollUpdate = change.try_into()?;

    let storage = storage!();
    let mut data: Data =
        serde_json::from_str(&storage.get_item("data")?.ok_or("No data in storage !")?)
            .map_err(|e| format!("{e}"))?;

    data.roll.update(validated);

    storage.set_item(
        "data",
        &serde_json::to_string(&data).map_err(|e| format!("{e}"))?,
    )
}

/* Deprecated
fn clone_row(index: u32) -> JsResult {
    let storage = storage!();
    let data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    let mut exposures: Vec<(u32, ExposureSpecificData)> = data.exposures.into_iter().collect();
    exposures.sort_by_key(|e| e.0);

    let current = exposures.iter_mut().position(|k| k.0 == index);

    let position = current.ok_or("No matching exposition")?;
    if position + 1 >= exposures.len() {
        Err("Cannot clone last row !")?
    }

    let (_, exp) = exposures[position].clone();
    let (target, _) = exposures[position + 1].clone();
    exposures[position + 1] = (target, exp.clone());
    exposure_update(target, exp)
}
*/

fn exposure_update(index: u32, exp: ExposureSpecificData) -> JsResult {
    let storage = storage!();
    let mut data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    [
        UIExposureUpdate::ShutterSpeed(exp.sspeed.clone().unwrap_or_default()),
        UIExposureUpdate::Aperture(exp.aperture.clone().unwrap_or_default()),
        UIExposureUpdate::Comment(exp.comment.clone().unwrap_or_default()),
        UIExposureUpdate::Lens(exp.lens.clone().unwrap_or_default()),
        UIExposureUpdate::Date(
            exp.date
                .map(|d| format!("{}", d.format("%Y-%m-%dT%H:%M:%S")))
                .unwrap_or_default(),
        ),
        UIExposureUpdate::Gps(
            exp.gps
                .map(|(lat, lon)| format!("{lat}, {lon}"))
                .unwrap_or_default(),
        ),
    ]
    .iter()
    .map(|c| update_exposure_ui(index, c))
    .collect::<Result<Vec<_>, _>>()?;

    data.exposures.insert(index, exp);

    storage.set_item(
        "data",
        &serde_json::to_string(&data).map_err(|e| e.to_string())?,
    )
}

pub fn get_selection() -> JsResult<Selection> {
    serde_json::from_str(&storage!().get_item("selected")?.unwrap_or_default())
        .map_err(|e| e.to_string())
        .or_else(|_| Ok(Selection::default()))
}

fn toggle_selection(index: u32, shift: bool, ctrl: bool) -> JsResult {
    let mut selection = get_selection()?;

    match (ctrl, shift) {
        (false, false) => {
            if !selection.contains(index) {
                selection.set_one(index)
            }
        }
        (true, false) => selection.toggle(index),
        (false, true) => selection.group_select(index),
        _ => (),
    }

    let choices = selection.items();
    for exposure in 0..=MAX_EXPOSURES {
        set_exposure_selection(exposure, choices.contains(&exposure)).ok();
    }

    storage!().set_item(
        "selected",
        &serde_json::to_string(&selection).map_err(|e| e.to_string())?,
    )
}

fn manage_selection(operation: Update) -> JsResult {
    let mut selection = get_selection()?;

    match operation {
        Update::SelectionInvert => selection.invert(),
        Update::SelectionClear => selection.clear(),
        Update::SelectionAll => selection.all(),
        _ => unreachable!(),
    }

    let choices = selection.items();
    for exposure in 0..=MAX_EXPOSURES {
        set_exposure_selection(exposure, choices.contains(&exposure)).ok();
    }

    storage!().set_item(
        "selected",
        &serde_json::to_string(&selection).map_err(|e| e.to_string())?,
    )
}

pub fn update(event: Update) -> JsResult {
    match event {
        Update::Roll(d) => roll_update(d),
        Update::Exposure(i, d) => exposure_update(i, d),
        Update::ExposureField(i, d) => exposure_update_field(i, d),
        Update::ExposureImage(i, d) => {
            wasm_bindgen_futures::spawn_local(async move {
                exposure_update_image(i, d)
                    .await
                    .inspect_err(|e| log::error!("{e}"))
                    .ok();
            });

            Ok(())
        }
        Update::ExposureImageRestore(i) => exposure_restore_image(i),
        Update::SelectExposure(e, shift, ctrl) => toggle_selection(e, shift, ctrl),
        Update::SelectionClear | Update::SelectionAll | Update::SelectionInvert => {
            manage_selection(event)
        }
    }
}

fn update_exposure_ui(index: u32, data: &UIExposureUpdate) -> JsResult {
    let (id, contents) = match data {
        UIExposureUpdate::ShutterSpeed(value) => (&format!("exposure-input-sspeed-{index}"), value),
        UIExposureUpdate::Aperture(value) => (&format!("exposure-input-aperture-{index}"), value),
        UIExposureUpdate::Comment(value) => (&format!("exposure-input-comment-{index}"), value),
        UIExposureUpdate::Date(value) => (&format!("exposure-input-date-{index}"), value),
        UIExposureUpdate::Lens(value) => (&format!("exposure-input-lens-{index}"), value),
        UIExposureUpdate::Gps(value) => (&format!("exposure-input-gps-{index}"), value),
    };

    query_id!(id, web_sys::HtmlInputElement).set_value(contents);
    Ok(())
}
