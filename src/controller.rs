use crate::models::{Data, ExposureSpecificData, Selection};
use crate::{set_exposure_selection, update_exposure_image, update_exposure_ui, JsResult};
use chrono::NaiveDateTime;
use std::convert::TryInto;

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
    ISO(String),
    Film(String),
}

#[derive(Debug, Clone)]
pub enum RollUpdate {
    Author(Option<String>),
    Make(Option<String>),
    Model(Option<String>),
    ISO(Option<String>),
    Film(Option<String>),
}

#[derive(Debug, Clone)]
pub enum UIExposureUpdate {
    ShutterSpeed(String),
    Aperture(String),
    Lens(String),
    Comment(String),
    Date(String),
    GPS(String),
}

#[derive(Debug, Clone)]
pub enum ExposureUpdate {
    ShutterSpeed(Option<String>),
    Aperture(Option<String>),
    Lens(Option<String>),
    Comment(Option<String>),
    Date(Option<NaiveDateTime>),
    GPS(Option<(f64, f64)>),
}

impl TryInto<ExposureUpdate> for UIExposureUpdate {
    type Error = wasm_bindgen::prelude::JsValue;

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
            UIExposureUpdate::GPS(value) => {
                let split = value.split(",").collect::<Vec<_>>();
                if split.len() == 2 {
                    match (
                        split[0].trim().parse::<f64>(),
                        split[1].trim().parse::<f64>(),
                    ) {
                        (Ok(lat), Ok(lon)) => ExposureUpdate::GPS(Some((lat, lon))),
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
            UIRollUpdate::ISO(s) => RollUpdate::ISO(if !s.is_empty() { Some(s) } else { None }),
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

    let selection: Selection =
        serde_json::from_str(&storage!().get_item("selected")?.unwrap_or("".into()))
            .map_err(|e| e.to_string())
            .unwrap_or_default();

    let mut selection = selection.items;
    selection.insert(index);

    for target in selection {
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

fn exposure_update_image(index: u32, data: String) -> JsResult {
    let storage = storage!();
    let mut image_cache: std::collections::HashMap<u32, String> =
        serde_json::from_str(&storage.get_item("image_cache")?.unwrap_or("{}".into()))
            .map_err(|e| format!("{e}"))?;

    update_exposure_image(index, &data)?;

    image_cache.insert(index, data);
    storage.set_item(
        "image_cache",
        &serde_json::to_string(&image_cache).map_err(|e| format!("{e}"))?,
    )
}

fn exposure_restore_image(index: u32) -> JsResult {
    let image_cache: std::collections::HashMap<u32, String> =
        serde_json::from_str(&storage!().get_item("image_cache")?.unwrap_or("{}".into()))
            .map_err(|e| format!("{e}"))?;

    let data = image_cache
        .get(&index)
        .ok_or(format!("No image cached for exposure {index}"))?;

    update_exposure_image(index, data)
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

fn exposure_update(index: u32, exp: ExposureSpecificData) -> JsResult {
    let storage = storage!();
    let mut data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    vec![
        UIExposureUpdate::ShutterSpeed(exp.sspeed.clone().unwrap_or_default()),
        UIExposureUpdate::Aperture(exp.aperture.clone().unwrap_or_default()),
        UIExposureUpdate::Comment(exp.comment.clone().unwrap_or_default()),
        UIExposureUpdate::Lens(exp.lens.clone().unwrap_or_default()),
        UIExposureUpdate::Date(
            exp.date
                .map(|d| format!("{}", d.format("%Y-%m-%dT%H:%M:%S")))
                .unwrap_or_default(),
        ),
        UIExposureUpdate::GPS(
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
    let storage = storage!();
    let mut selection = get_selection()?;

    set_exposure_selection(index, selection.toggle(index))?;

    storage.set_item(
        "selected",
        &serde_json::to_string(&selection).map_err(|e| e.to_string())?,
    )
}

fn manage_selection<F: Fn(bool) -> bool>(choice: F) -> JsResult {
    let storage = storage!();
    let data: Data = serde_json::from_str(&storage.get_item("data")?.ok_or("No data")?)
        .map_err(|e| e.to_string())?;

    let selection = get_selection()?;

    // TODO fix
    let selection = data
        .exposures
        .keys()
        .filter_map(|i| {
            let new = choice(selection.contains(*i));
            set_exposure_selection(*i, new).ok();
            new.then_some(i)
        })
        .collect::<Vec<_>>();

    storage.set_item(
        "selected",
        &serde_json::to_string(&selection).map_err(|e| e.to_string())?,
    )
}

pub fn update(event: Update) -> JsResult {
    match event {
        Update::Roll(d) => roll_update(d),
        Update::Exposure(i, d) => exposure_update(i, d),
        Update::ExposureField(i, d) => exposure_update_field(i, d),
        Update::ExposureImage(i, d) => exposure_update_image(i, d),
        Update::ExposureImageRestore(i) => exposure_restore_image(i),
        Update::SelectExposure(e, shift, ctrl) => toggle_selection(e, shift, ctrl),
        Update::SelectionClear => manage_selection(|_| false),
        Update::SelectionAll => manage_selection(|_| true),
        Update::SelectionInvert => manage_selection(|s| !s),
    }
}
