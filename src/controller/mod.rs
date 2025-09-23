use crate::{
    Aquiesce, Error, Orientation,
    models::{
        Data, ExposureSpecificData, FileMetadata, HTML_INPUT_TIMESTAMP_FORMAT,
        HTML_INPUT_TIMESTAMP_FORMAT_N, History, ReorderMetadataExt, RollData, Selection,
    },
    view,
};
use chrono::NaiveDateTime;
use std::{cell::RefCell, convert::TryInto};

mod local_storage;
mod notifications;

pub use local_storage::{
    clear as clear_local_storage, get_data, get_metadata, get_selection, get_tse, set_data,
    set_metadata, set_selection,
};
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
    FileMetadata(FileMetadata),
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
    let mut data = get_data()?;
    let backup = data.clone();

    for target in get_selection()?.items() {
        data.exposures
            .get_mut(&target)
            .ok_or(Error::MissingKey(format!("exposure {target}")))?
            .update(change.clone().try_into()?);
    }

    HISTORY.with_borrow_mut(|history| history.record(backup));
    view::exposure::allow_undo(true)?;

    set_data(&data)?;

    if let UIExposureUpdate::GpsMap(lat, lng) | UIExposureUpdate::Gps(lat, lng) = change {
        view::map::show_location(&[(lat, lng)]);
    }

    if let UIExposureUpdate::GpsMap(lat, lng) = change {
        view::exposure::set_gps_input_contents(&format!("{lat}, {lng}"))?;
    }

    Ok(())
}

pub fn generate_folder_name() -> Result<String, Error> {
    let data = get_data()?;

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

fn show_selection(selection: &Selection) -> Result<(), Error> {
    let data = get_data()?;

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
    let mut selection = get_selection().unwrap_or_default();
    let data = get_metadata()?;
    let all: Selection = data.into_iter().map(|m| m.index).collect();

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

    set_selection(&selection)?;

    show_selection(&selection)
        .and(view::preview::reflect_selection(&all, &selection))
        .aquiesce();

    Ok(())
}

fn undo() -> Result<(), Error> {
    let selection = get_selection()?;

    if let Some(data) = HISTORY.with_borrow_mut(|h| h.pop()) {
        set_data(&data)?
    }

    show_selection(&selection).aquiesce();
    view::exposure::allow_undo(HISTORY.with_borrow(|h| h.undoable()))?;

    Ok(())
}

fn rotate(update: Update) -> Result<(), Error> {
    let selection = get_selection()?;
    let mut metadata = get_metadata()?;

    let rotation = match update {
        Update::RotateLeft => Orientation::Rotated270,
        Update::RotateRight => Orientation::Rotated90,
        _ => unreachable!(),
    };

    for index in selection.items() {
        metadata
            .iter_mut()
            .find(|m| m.index == index)
            .map(|e| e.orientation = e.orientation.rotate(rotation))
            .ok_or(Error::MissingKey(format!("exposure {index}")))?;

        view::preview::rotate_thumbnail(index, rotation).aquiesce();
    }

    set_metadata(&metadata)
}

fn reorder(new: u32) -> Result<(), Error> {
    let items = get_selection()?.items();
    assert!(items.len() == 1);
    let old = items.first().expect("Bad selection");

    let metadata = get_metadata()?.reorder(*old, new);

    set_metadata(&metadata)?;

    Ok(())
}

pub fn update(event: Update) -> Result<(), Error> {
    match event {
        Update::Roll(change) => {
            let mut data = get_data()?;

            data.roll.update(change.into());

            set_data(&data)
        }
        Update::Exposure(d) => exposure_update_field(d),
        Update::SelectExposure(_, _, _)
        | Update::SelectionClear
        | Update::SelectionAll
        | Update::SelectionInvert => manage_selection(event),
        Update::FileMetadata(data) => {
            let mut metadata = get_metadata().unwrap_or_default();

            match metadata.iter_mut().find(|m| m.index == data.index) {
                Some(entry) => *entry = data,
                None => metadata.push(data),
            }

            set_metadata(&metadata)
        }
        Update::RotateLeft | Update::RotateRight => rotate(event),
        Update::Undo => undo(),
    }
}
