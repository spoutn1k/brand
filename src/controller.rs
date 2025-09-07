use crate::{
    Aquiesce, Error, Orientation, QueryExt, SessionStorageExt,
    models::{
        Data, ExposureSpecificData, FileMetadata, HTML_INPUT_TIMESTAMP_FORMAT,
        HTML_INPUT_TIMESTAMP_FORMAT_N, Meta, RollData, Selection, WorkerCompressionAnswer,
    },
    storage, view,
};
use async_channel::{Receiver, Sender};
use chrono::NaiveDateTime;
use std::{cell::Cell, convert::TryInto, path::PathBuf};
use winnow::{ModalResult, Parser, ascii::float, combinator::separated_pair};

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
    Exposure(UIExposureUpdate),
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
pub enum UIExposureUpdate {
    ShutterSpeed(String),
    Aperture(String),
    Lens(String),
    Comment(String),
    Date(String),
    Gps(String),
}

fn parse_gps(r: String) -> Result<(f64, f64), Error> {
    fn inner(line: &mut &str) -> ModalResult<(f64, f64)> {
        separated_pair(float, ",", float).parse_next(line)
    }

    let pair = inner
        .parse(r.as_str())
        .map_err(|e| Error::GpsParse(e.to_string()))?;

    Ok(pair)
}

impl TryFrom<UIExposureUpdate> for ExposureSpecificData {
    type Error = Error;

    fn try_from(update: UIExposureUpdate) -> Result<Self, Error> {
        let mut data = Self::default();

        match update {
            UIExposureUpdate::ShutterSpeed(s) => data.sspeed = (!s.is_empty()).then_some(s),
            UIExposureUpdate::Aperture(s) => data.aperture = (!s.is_empty()).then_some(s),
            UIExposureUpdate::Comment(s) => data.aperture = (!s.is_empty()).then_some(s),
            UIExposureUpdate::Lens(s) => data.aperture = (!s.is_empty()).then_some(s),
            UIExposureUpdate::Date(s) => {
                data.date = (!s.is_empty()).then_some(
                    NaiveDateTime::parse_from_str(&s, HTML_INPUT_TIMESTAMP_FORMAT).or(
                        NaiveDateTime::parse_from_str(&s, HTML_INPUT_TIMESTAMP_FORMAT_N),
                    )?,
                )
            }
            UIExposureUpdate::Gps(value) => data.gps = Some(parse_gps(value)?),
        }

        Ok(data)
    }
}

impl From<UIRollUpdate> for RollData {
    fn from(update: UIRollUpdate) -> Self {
        let mut data = Self::default();

        match update {
            UIRollUpdate::Author(s) => data.author = (!s.is_empty()).then_some(s),
            UIRollUpdate::Make(s) => data.make = (!s.is_empty()).then_some(s),
            UIRollUpdate::Model(s) => data.model = (!s.is_empty()).then_some(s),
            UIRollUpdate::Iso(s) => data.iso = (!s.is_empty()).then_some(s),
            UIRollUpdate::Film(s) => data.description = (!s.is_empty()).then_some(s),
        };

        data
    }
}

fn exposure_update_field(change: UIExposureUpdate) -> Result<(), Error> {
    let storage = storage()?;
    let mut data: Data = serde_json::from_str(&storage.get_existing("data")?)?;

    for target in get_selection()?.items() {
        data.exposures
            .get_mut(&target)
            .ok_or(Error::MissingKey(format!("exposure {target}")))?
            .update(change.clone().try_into()?);
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

    show_selection(&selection)
        .and(view::preview::reflect_selection(&all, &selection))
        .aquiesce();

    Ok(())
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
