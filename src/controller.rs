use crate::{
    Aquiesce, Error, JsResult, Orientation,
    macros::{MacroError, SessionStorageExt, query_id, storage},
    models::{
        Data, ExposureData, ExposureSpecificData, FileMetadata, MAX_EXPOSURES, Meta, Selection,
    },
};
use base64::prelude::*;
use chrono::NaiveDateTime;
use image::{ImageReader, codecs::jpeg::JpegEncoder, imageops::FilterType};
use std::{convert::TryInto, io::Cursor, path::PathBuf};
use wasm_bindgen::{JsCast, JsValue};

pub enum Update {
    ExposureImage(u32),
    SelectExposure(u32, bool, bool),
    SelectionClear,
    SelectionAll,
    SelectionInvert,
    Exposure(u32, ExposureSpecificData),
    ExposureField(u32, UIExposureUpdate),
    Roll(UIRollUpdate),
    FileMetadata(PathBuf, FileMetadata),
    ExposureRotate(u32, Orientation),
    RotateLeft,
    //RotatedRight,
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

fn exposure_update_field(index: u32, change: UIExposureUpdate) -> Result<(), Error> {
    let validated: ExposureUpdate = change.clone().try_into()?;

    let storage = storage!();
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    let selection: Selection = get_selection()?;

    for target in std::iter::once(index).chain(selection.items()) {
        data.exposures
            .get_mut(&target)
            .ok_or(Error::MissingKey(format!("exposure {target}")))?
            .update(validated.clone());

        if target != index {
            update_exposure_ui(target, &change)?;
        }
    }

    update_exposure_ui(index, &change)?;

    storage.set_item("data", &serde_json::to_string(&data)?)?;

    Ok(())
}

fn set_exposure_selection(index: u32, selected: bool) -> Result<(), Error> {
    query_id!(
        &format!("exposure-input-select-{index}"),
        web_sys::HtmlInputElement
    )
    .set_checked(selected);

    let classes = query_id!(&format!("exposure-{index}")).class_list();
    if selected {
        query_id!(&format!("exposure-{index}-preview")).remove_attribute("hidden")?;

        classes.add_1("selected")?;
    } else {
        query_id!(&format!("exposure-{index}-preview")).set_attribute("hidden", "true")?;

        classes.remove_1("selected")?;
    }

    Ok(())
}

async fn exposure_update_image(index: u32) -> Result<(), Error> {
    let storage = storage!();
    let data: Meta = serde_json::from_str(&storage.get_existing("metadata")?)?;

    let (_, meta) = data
        .into_iter()
        .find(|(_, meta)| meta.index == index)
        .ok_or(Error::MissingKey(format!(
            "No image path for exposure {index}"
        )))?;

    let file = meta.local_fs_path.ok_or(Error::MissingKey(format!(
        "Missing local file for exposure {index}"
    )))?;

    log::info!(
        "Updating image for exposure {index} with file {}",
        file.display()
    );

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

    query_id!(&format!("exposure-{index}-preview"))
        .set_attribute("src", &format!("data:image/jpeg;base64, {base64}"))?;

    Ok(())
}

fn roll_update(change: UIRollUpdate) -> Result<(), Error> {
    let validated: RollUpdate = change.try_into()?;

    let storage = storage!();
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    data.roll.update(validated);

    storage.set_item("data", &serde_json::to_string(&data)?)?;

    Ok(())
}

pub fn exposure_update(index: u32, exp: ExposureSpecificData) -> Result<(), Error> {
    [
        UIExposureUpdate::ShutterSpeed(exp.sspeed.clone().unwrap_or_default()),
        UIExposureUpdate::Aperture(exp.aperture.clone().unwrap_or_default()),
        UIExposureUpdate::Comment(exp.comment.clone().unwrap_or_default()),
        UIExposureUpdate::Lens(exp.lens.clone().unwrap_or_default()),
        UIExposureUpdate::Date(
            exp.date
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
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

    Ok(())
}

pub fn overhaul_data(contents: Data) -> Result<(), Error> {
    [
        UIRollUpdate::Author(contents.roll.author.clone().unwrap_or_default()),
        UIRollUpdate::Make(contents.roll.make.clone().unwrap_or_default()),
        UIRollUpdate::Model(contents.roll.model.clone().unwrap_or_default()),
        UIRollUpdate::Iso(contents.roll.iso.clone().unwrap_or_default()),
        UIRollUpdate::Film(contents.roll.description.clone().unwrap_or_default()),
    ]
    .iter()
    .map(update_roll_ui)
    .collect::<Result<Vec<_>, _>>()?;

    for (index, exp) in contents.exposures.into_iter() {
        exposure_update(index, exp).aquiesce();
    }

    Ok(())
}

pub fn generate_folder_name() -> Result<String, Error> {
    let data: Data = serde_json::from_str(&storage!().get_existing("data")?)?;

    let min = data
        .exposures
        .keys()
        .min()
        .and_then(|&k| data.exposures.get(&k))
        .and_then(|e| e.date)
        .map(|d| d.format("%Y%m").to_string());

    let max = data
        .exposures
        .keys()
        .max()
        .and_then(|&k| data.exposures.get(&k))
        .and_then(|e| e.date)
        .map(|d| d.format("%Y%m").to_string());

    let mut folder_name = match (min, max) {
        (Some(min), Some(max)) if min == max => min,
        (Some(min), Some(max)) => format!("{}_{}", min, max),
        (Some(min), None) => min,
        (None, Some(max)) => max,
        (None, None) => "".into(),
    };

    if let Some(film) = data
        .roll
        .description
        .and_then(|f| f.split(" ").last().map(String::from))
    {
        folder_name = format!("{folder_name}_{film}");
    }

    Ok(folder_name)
}

pub fn get_selection() -> Result<Selection, Error> {
    serde_json::from_str(&storage!().get_item("selected")?.unwrap_or_default())
        .or_else(|_| Ok(Selection::default()))
}

pub fn get_exposure_data(index: u32) -> Result<ExposureData, Error> {
    let storage = storage!();
    let data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    Ok(data.spread_shots().generate(index))
}

fn toggle_selection(index: u32, shift: bool, ctrl: bool) -> Result<(), Error> {
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

    storage!().set_item("selected", &serde_json::to_string(&selection)?)?;

    Ok(())
}

fn manage_selection(operation: Update) -> Result<(), Error> {
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

    storage!().set_item("selected", &serde_json::to_string(&selection)?)?;

    Ok(())
}

fn rotate_left_selection() -> Result<(), Error> {
    let selection = get_selection()?;

    for index in selection.items() {
        rotate_id(index, Orientation::Rotated270)?;
    }

    Ok(())
}

fn rotate_id(index: u32, orientation: Orientation) -> Result<(), Error> {
    let storage = storage!();
    let mut data: Meta = serde_json::from_str(&storage.get_existing("metadata")?)?;

    let (p, m) = data
        .iter()
        .find(|(_, m)| m.index == index)
        .ok_or(Error::MissingKey(format!("exposure {index}")))?;

    let mut meta = m.clone();
    meta.orientation = meta.orientation.rotate(orientation);
    data.insert(p.clone(), meta);

    storage.set_item("metadata", &serde_json::to_string(&data)?)?;

    wasm_bindgen_futures::spawn_local(async move {
        exposure_update_image(index).await.aquiesce();
    });

    Ok(())
}

pub async fn update(event: Update) -> Result<(), Error> {
    match event {
        Update::Roll(d) => roll_update(d),
        Update::Exposure(i, d) => exposure_update(i, d),
        Update::ExposureField(i, d) => exposure_update_field(i, d),
        Update::ExposureImage(i) => {
            exposure_update_image(i).await.aquiesce();

            Ok(())
        }
        Update::SelectExposure(e, shift, ctrl) => toggle_selection(e, shift, ctrl),
        Update::SelectionClear | Update::SelectionAll | Update::SelectionInvert => {
            manage_selection(event)
        }
        Update::FileMetadata(path, metadata) => {
            let storage = storage!();
            let mut data: Meta = serde_json::from_str(&storage.get_existing("metadata")?)?;

            data.insert(path, metadata);

            storage.set_item("metadata", &serde_json::to_string(&data)?)?;

            Ok(())
        }
        Update::ExposureRotate(index, orientation) => rotate_id(index, orientation),
        Update::RotateLeft => rotate_left_selection(),
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

fn update_roll_ui(data: &UIRollUpdate) -> JsResult {
    let (id, contents) = match data {
        UIRollUpdate::Author(value) => (&format!("roll-author-input"), value),
        UIRollUpdate::Make(value) => (&format!("roll-make-input"), value),
        UIRollUpdate::Model(value) => (&format!("roll-model-input"), value),
        UIRollUpdate::Iso(value) => (&format!("roll-iso-input"), value),
        UIRollUpdate::Film(value) => (&format!("roll-description-input"), value),
    };

    query_id!(id, web_sys::HtmlInputElement).set_value(contents);

    Ok(())
}
