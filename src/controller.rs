use crate::{
    Aquiesce, Error, Orientation, QueryExt, SessionStorageExt,
    models::{
        Data, ExposureData, FileMetadata, HTML_INPUT_TIMESTAMP_FORMAT,
        HTML_INPUT_TIMESTAMP_FORMAT_N, Meta, Selection, WorkerCompressionAnswer,
    },
    storage, view,
};
use async_channel::{Receiver, Sender};
use chrono::NaiveDateTime;
use std::{cell::Cell, convert::TryInto, path::PathBuf};

thread_local! {
static CHANNEL: (Sender<Progress>, Receiver<Progress>) = async_channel::bounded(80);
static THUMBNAIL_TRACKER: Cell<(u32, u32)> = Cell::new((0, 0));
static PROCESSING_TRACKER: Cell<(u32, u32)> = Cell::new((0, 0));
}

#[derive(Debug)]
pub enum Update {
    SelectExposure(u32, bool, bool),
    SelectionClear,
    SelectionAll,
    SelectionInvert,
    ExposureField(UIExposureUpdate),
    Roll(UIRollUpdate),
    FileMetadata(PathBuf, FileMetadata),
    RotateLeft,
    RotateRight,
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
    type Error = Error;

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
                NaiveDateTime::parse_from_str(&value, HTML_INPUT_TIMESTAMP_FORMAT).or(
                    NaiveDateTime::parse_from_str(&value, HTML_INPUT_TIMESTAMP_FORMAT_N),
                )?,
            )),
            UIExposureUpdate::Gps(value) => {
                let split = value.split(",").collect::<Vec<_>>();
                if split.len() == 2 {
                    match (
                        split[0].trim().parse::<f64>(),
                        split[1].trim().parse::<f64>(),
                    ) {
                        (Ok(lat), Ok(lon)) => ExposureUpdate::Gps(Some((lat, lon))),
                        (Err(_), _) => Err(Error::GpsParse(split[0].to_string()))?,
                        (_, Err(_)) => Err(Error::GpsParse(split[1].to_string()))?,
                    }
                } else {
                    Err(Error::GpsParse(value))?
                }
            }
        })
    }
}

impl TryInto<RollUpdate> for UIRollUpdate {
    type Error = Error;

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

fn exposure_update_field(change: UIExposureUpdate) -> Result<(), Error> {
    let validated: ExposureUpdate = change.clone().try_into()?;

    let storage = storage()?;
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    for target in get_selection()?.items() {
        data.exposures
            .get_mut(&target)
            .ok_or(Error::MissingKey(format!("exposure {target}")))?
            .update(validated.clone());
    }

    storage.set_item("data", &serde_json::to_string(&data)?)?;

    Ok(())
}

async fn exposure_update_image(meta: FileMetadata) -> Result<(), Error> {
    let WorkerCompressionAnswer(index, base64) = crate::worker::compress_image(meta).await?;

    format!("exposure-{index}-preview")
        .query_id()?
        .set_attribute("src", &format!("data:image/jpeg;base64, {base64}"))?;

    Ok(())
}

fn roll_update(change: UIRollUpdate) -> Result<(), Error> {
    let validated: RollUpdate = change.try_into()?;

    let storage = storage()?;
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    data.roll.update(validated);

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

pub fn get_exposure_data(index: u32) -> Result<ExposureData, Error> {
    let storage = storage()?;
    let data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    Ok(data.spread_shots().generate(index))
}

fn show_selection(selection: &Selection) -> Result<(), Error> {
    let data: Data = storage()?
        .get_item("data")?
        .ok_or(Error::MissingKey("Data".into()))
        .and_then(|s| serde_json::from_str(&s).map_err(Error::from))
        .unwrap_or_default();

    match selection.items().last() {
        Some(index) => {
            view::exposure::set_contents(
                format!("Exposure {selection}"),
                &data.exposures.get(&index).cloned().unwrap_or_default(),
            )?;
            view::roll::hide()?;
            view::exposure::show()
        }
        None => view::roll::show().and(view::exposure::hide()),
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

    show_selection(&selection)?;
    view::preview::reflect_selection(&all, &selection)
}

pub fn rotate(update: Update) -> Result<(), Error> {
    let selection = get_selection()?;

    for index in selection.items() {
        match update {
            Update::RotateLeft => rotate_id(index, Orientation::Rotated270)?,
            Update::RotateRight => rotate_id(index, Orientation::Rotated90)?,
            _ => unreachable!(),
        }
    }

    Ok(())
}

fn rotate_id(index: u32, orientation: Orientation) -> Result<(), Error> {
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

    wasm_bindgen_futures::spawn_local(async move {
        exposure_update_image(meta).await.aquiesce();
    });

    Ok(())
}

pub fn update(event: Update) -> Result<(), Error> {
    match event {
        Update::Roll(d) => roll_update(d),
        Update::ExposureField(d) => exposure_update_field(d),
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
        Update::RotateLeft | Update::RotateRight => rotate(event),
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

pub fn notifier() -> Sender<Progress> {
    CHANNEL.with(|t| t.0.clone())
}

pub fn sender() -> Receiver<Progress> {
    CHANNEL.with(|t| t.1.clone())
}

pub async fn handle_progress() -> Result<(), Error> {
    let thumbnails = "thumbnails".query_id()?;
    let processing = "processing".query_id()?;

    while let Ok(data) = sender().recv().await {
        match data {
            Progress::ThumbnailStart(count) => {
                THUMBNAIL_TRACKER.set((0, count));
                thumbnails.set_text_content(Some(&format!("Generating thumbnails (0/{count})")));
            }
            Progress::ThumbnailGenerated(_) => {
                let (done, count) = THUMBNAIL_TRACKER.get();
                let done = done + 1;
                THUMBNAIL_TRACKER.set((done, count));
                thumbnails
                    .set_text_content(Some(&format!("Generating thumbnails ({done}/{count})")));
            }
            Progress::ThumbnailDone => thumbnails.set_text_content(Some("")),
            Progress::ProcessingStart(count) => {
                PROCESSING_TRACKER.set((0, count));
                processing.set_text_content(Some(&format!("Processing (0/{count})")));
            }
            Progress::Processing(_) => {
                let (done, count) = PROCESSING_TRACKER.get();
                let done = done + 1;
                PROCESSING_TRACKER.set((done, count));
                processing.set_text_content(Some(&format!("Processing ({done}/{count})")));
            }
            Progress::ProcessingDone => processing.set_text_content(Some("")),
        }
    }

    Ok(())
}
