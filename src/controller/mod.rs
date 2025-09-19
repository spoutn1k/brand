use crate::{
    Aquiesce, Error, Orientation, QueryExt, SessionStorageExt,
    models::{
        Data, ExposureSpecificData, FileMetadata, HTML_INPUT_TIMESTAMP_FORMAT,
        HTML_INPUT_TIMESTAMP_FORMAT_N, History, Meta, RollData, Selection, TseFormat,
    },
    storage, view, worker,
    worker::WorkerCompressionAnswer,
};
use chrono::NaiveDateTime;
use std::{cell::RefCell, convert::TryInto, path::PathBuf};

mod notifications;

pub use notifications::{Progress, handle_progress, notify};

thread_local! {
    static HISTORY: RefCell<History<Data>> = RefCell::new(Default::default());
}

#[derive(Debug, Clone)]
pub enum Update {
    SelectExposure(u32, bool, bool),
    SelectionClear,
    SelectionAll,
    SelectionInvert,
    Exposure(UIExposureUpdate),
    Roll(UIRollUpdate),
    FileMetadata(PathBuf, FileMetadata),
    RotateLeft,
    RotateRight,
    Undo,
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
pub enum UIExposureUpdate {
    ShutterSpeed(String),
    Aperture(String),
    Lens(String),
    Comment(String),
    Date(String),
    Gps(f64, f64),
    GpsMap(f64, f64),
}

impl TryFrom<UIExposureUpdate> for ExposureSpecificData {
    type Error = Error;

    fn try_from(update: UIExposureUpdate) -> Result<Self, Error> {
        let mut data = Self::default();

        match update {
            UIExposureUpdate::ShutterSpeed(s) => data.sspeed = Some(s),
            UIExposureUpdate::Aperture(s) => data.aperture = Some(s),
            UIExposureUpdate::Comment(s) => data.comment = Some(s),
            UIExposureUpdate::Lens(s) => data.lens = Some(s),
            UIExposureUpdate::Date(s) => {
                data.date = NaiveDateTime::parse_from_str(&s, HTML_INPUT_TIMESTAMP_FORMAT)
                    .or(NaiveDateTime::parse_from_str(
                        &s,
                        HTML_INPUT_TIMESTAMP_FORMAT_N,
                    ))
                    .inspect_err(|e| log::error!("{e}"))
                    .ok()
            }
            UIExposureUpdate::Gps(lat, lng) | UIExposureUpdate::GpsMap(lat, lng) => {
                data.gps = Some((lat, lng))
            }
        }

        Ok(data)
    }
}

impl From<UIRollUpdate> for RollData {
    fn from(update: UIRollUpdate) -> Self {
        let mut data = Self::default();

        match update {
            UIRollUpdate::Author(s) => data.author = Some(s),
            UIRollUpdate::Make(s) => data.make = Some(s),
            UIRollUpdate::Model(s) => data.model = Some(s),
            UIRollUpdate::Iso(s) => data.iso = Some(s),
            UIRollUpdate::Film(s) => data.description = Some(s),
        };

        data
    }
}

fn exposure_update_field(change: UIExposureUpdate) -> Result<(), Error> {
    let storage = storage()?;
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;
    let backup = data.clone();

    for target in get_selection()?.items() {
        data.exposures
            .get_mut(&target)
            .ok_or(Error::MissingKey(format!("exposure {target}")))?
            .update(change.clone().try_into()?);
    }

    HISTORY.with_borrow_mut(|history| history.record(backup));
    view::exposure::allow_undo(HISTORY.with_borrow(|h| h.undoable()))?;

    storage.set_item("data", &serde_json::to_string(&data)?)?;

    if let UIExposureUpdate::GpsMap(lat, lng) | UIExposureUpdate::Gps(lat, lng) = change {
        view::map::show_location(&[(lat, lng)]);
    }

    if let UIExposureUpdate::GpsMap(lat, lng) = change {
        view::exposure::set_gps_input_contents(&format!("{lat}, {lng}"))?;
    }

    Ok(())
}

async fn exposure_update_image(meta: FileMetadata) -> Result<(), Error> {
    let WorkerCompressionAnswer(index, base64) = worker::compress_image(meta).await?;

    format!("exposure-{index}-preview")
        .query_id()?
        .set_attribute("src", &format!("data:image/jpeg;base64, {base64}"))?;

    Ok(())
}

fn roll_update(change: UIRollUpdate) -> Result<(), Error> {
    let storage = storage()?;
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    data.roll.update(change.into());

    storage.set_item("data", &serde_json::to_string(&data)?)?;

    Ok(())
}

pub fn generate_folder_name() -> Result<String, Error> {
    let data: Data = serde_json::from_str(&storage()?.get_existing("data")?)?;

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
    serde_json::from_str(&storage()?.get_item("selected")?.unwrap_or_default())
        .or(Ok(Selection::default()))
}

pub fn get_exposure_data() -> Result<Data, Error> {
    let storage = storage()?;
    let data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    Ok(data.spread_shots())
}

fn show_selection(selection: &Selection) -> Result<(), Error> {
    let data: Data = storage()?
        .get_item("data")?
        .ok_or(Error::MissingKey("Data".into()))
        .and_then(|s| serde_json::from_str(&s).map_err(Error::from))
        .unwrap_or_default();

    match selection.items().len() {
        0 => view::roll::show().and(view::exposure::hide()),
        1 => {
            let index = selection.items().last().cloned().unwrap_or_default();
            view::exposure::one(
                index,
                &data.exposures.get(&index).cloned().unwrap_or_default(),
            )
            .and(view::roll::hide())
            .and(view::exposure::show())
        }
        _ => {
            let contents = selection
                .items()
                .iter()
                .filter_map(|i| data.exposures.get(i).cloned())
                .collect::<Vec<_>>();
            view::exposure::multiple(selection, contents.as_slice())
                .and(view::roll::hide())
                .and(view::exposure::show())
        }
    }
}

fn manage_selection(operation: Update) -> Result<(), Error> {
    let storage = storage()?;
    let mut selection = get_selection()?;
    let data: Meta = serde_json::from_str(&storage.get_existing("metadata")?)?;
    let all: Selection = data.into_values().map(|m| m.index).collect();

    let inverted: Selection = all
        .items()
        .into_iter()
        .filter(|i| !selection.contains(*i))
        .collect();

    match operation {
        Update::SelectionInvert => selection = inverted,
        Update::SelectionClear => selection.clear(),
        Update::SelectionAll => selection = all.clone(),
        Update::SelectExposure(index, shift, ctrl) => match (ctrl, shift) {
            (false, false) => {
                if !selection.contains(index) {
                    selection.set_one(index)
                }
            }
            (true, false) => selection.toggle(index),
            (false, true) => selection.group_select(index),
            _ => (),
        },
        _ => unreachable!(),
    }

    storage.set_item("selected", &serde_json::to_string(&selection)?)?;

    show_selection(&selection)
        .and(view::preview::reflect_selection(&all, &selection))
        .aquiesce();

    Ok(())
}

fn undo() -> Result<(), Error> {
    let storage = storage()?;
    let selection = get_selection()?;

    if let Some(data) = HISTORY.with_borrow_mut(|h| h.pop()) {
        storage.set_item("data", &serde_json::to_string(&data)?)?
    }

    show_selection(&selection).aquiesce();
    view::exposure::allow_undo(HISTORY.with_borrow(|h| h.undoable()))?;

    Ok(())
}

async fn rotate(update: Update) -> Result<(), Error> {
    let selection = get_selection()?;

    for index in selection.items() {
        match update {
            Update::RotateLeft => rotate_id(index, Orientation::Rotated270).await?,
            Update::RotateRight => rotate_id(index, Orientation::Rotated90).await?,
            _ => unreachable!(),
        }
    }

    Ok(())
}

async fn rotate_id(index: u32, orientation: Orientation) -> Result<(), Error> {
    let storage = storage()?;
    let mut data: Meta = serde_json::from_str(&storage.get_existing("metadata")?)?;

    let (p, m) = data
        .iter()
        .find(|(_, m)| m.index == index)
        .ok_or(Error::MissingKey(format!("exposure {index}")))?;

    let mut meta = m.clone();
    meta.orientation = meta.orientation.rotate(orientation);
    data.insert(p.clone(), meta.clone());

    storage.set_item("metadata", &serde_json::to_string(&data)?)?;

    exposure_update_image(meta).await?;

    Ok(())
}

pub fn update(event: Update) -> Result<(), Error> {
    match event {
        Update::Roll(d) => roll_update(d),
        Update::Exposure(d) => exposure_update_field(d),
        Update::SelectExposure(_, _, _)
        | Update::SelectionClear
        | Update::SelectionAll
        | Update::SelectionInvert => manage_selection(event),
        Update::FileMetadata(path, metadata) => {
            let storage = storage()?;
            let mut data: Meta = serde_json::from_str(&storage.get_existing("metadata")?)?;

            data.insert(path, metadata);

            storage.set_item("metadata", &serde_json::to_string(&data)?)?;

            Ok(())
        }
        Update::RotateLeft | Update::RotateRight => {
            wasm_bindgen_futures::spawn_local(async move {
                rotate(event).await.aquiesce();
            });

            Ok(())
        }
        Update::Undo => undo(),
    }
}

pub fn get_tse() -> Result<String, Error> {
    let data: Data = serde_json::from_str(&storage()?.get_existing("data")?)?;

    Ok(data.as_tse())
}
