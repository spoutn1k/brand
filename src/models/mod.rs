use crate::Error;
use chrono::{DateTime, NaiveDateTime};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    ops::Add,
    path::PathBuf,
};

mod history;
mod selection;
mod tse;

pub use history::History;
pub use selection::Selection;
pub use tse::{TseFormat, read_tse};

pub static HTML_INPUT_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
pub static HTML_INPUT_TIMESTAMP_FORMAT_N: &str = "%Y-%m-%dT%H:%M";

#[repr(u8)]
#[derive(Default, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Orientation {
    #[default]
    Normal = 0,
    Rotated90 = 1,
    Rotated180 = 2,
    Rotated270 = 3,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct FileMetadata {
    pub name: String,
    pub local_fs_path: PathBuf,
    pub index: u32,
    pub orientation: Orientation,
    pub file_type: FileKind,
}

#[derive(PartialEq, Eq, Default, Clone, Debug)]
pub enum FileKind {
    Image(ImageFormat),
    Tse,
    #[default]
    Unknown,
}

impl FileKind {
    pub fn is_tiff(&self) -> bool {
        match self {
            Self::Image(format) => *format == ImageFormat::Tiff,
            _ => false,
        }
    }
}

pub trait ValidateMetadataExt {
    fn validate(&self) -> Result<(), Error>;
}

impl ValidateMetadataExt for [FileMetadata] {
    fn validate(&self) -> Result<(), Error> {
        let mut paths = HashSet::new();
        let mut indexes = HashSet::new();

        for entry in self {
            paths.insert(&entry.name);
            indexes.insert(entry.index);
        }

        (paths.len() == indexes.len() && paths.len() == self.len())
            .then_some(())
            .ok_or(Error::InvalidMetadata)
    }
}

impl Serialize for FileKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            FileKind::Image(format) => format.to_mime_type(),
            FileKind::Tse => "tse",
            FileKind::Unknown => "unknown",
        })
    }
}

impl<'de> Deserialize<'de> for FileKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        if s == "tse" {
            return Ok(FileKind::Tse);
        }

        if s == "unknown" {
            return Ok(FileKind::Unknown);
        }

        ImageFormat::from_mime_type(&s)
            .map(FileKind::Image)
            .ok_or(serde::de::Error::custom(format!(
                "Unsupported image format: {s}"
            )))
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Data {
    pub roll: RollData,
    pub exposures: BTreeMap<u32, ExposureSpecificData>,
}

impl Data {
    pub fn with_count(count: u32) -> Self {
        let exposures = (1..=count)
            .map(|index| (index, ExposureSpecificData::default()))
            .collect();

        Self {
            exposures,
            ..Default::default()
        }
    }

    pub fn generate(&self, index: u32) -> ExposureData {
        let exposure = self.exposures.get(&index).cloned().unwrap_or_default();

        ExposureData {
            author: self.roll.author.clone(),
            make: self.roll.make.clone(),
            model: self.roll.model.clone(),
            iso: self.roll.iso.clone(),
            description: self.roll.description.clone(),
            sspeed: exposure.sspeed,
            aperture: exposure.aperture,
            lens: exposure.lens,
            comment: exposure.comment,
            date: exposure.date,
            gps: exposure.gps,
        }
    }

    pub fn spread_shots(self) -> Self {
        let Data { roll, exposures } = self;

        let mut exposures: Vec<_> = exposures.into_iter().collect();
        exposures.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
        let mut last = None;

        let exposures = exposures
            .into_iter()
            .map(|(i, mut data)| {
                match (data.date, last) {
                    (Some(timestamp), None) => last = Some((timestamp, 1)),
                    (Some(timestamp), Some((date, offset))) if timestamp == date => {
                        let step = DateTime::from_timestamp(date.and_utc().timestamp() + offset, 0)
                            .map(|d| d.naive_local())
                            .expect("Bad date generated");
                        last = Some((date, offset + 1));
                        data.date = Some(step);
                    }
                    (Some(timestamp), Some(_)) => last = Some((timestamp, 1)),
                    _ => (),
                };
                (i, data)
            })
            .collect();

        Data { roll, exposures }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct RollData {
    pub author: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub iso: Option<String>,
    pub description: Option<String>,
}

#[serde_with::serde_as]
#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct ExposureSpecificData {
    pub sspeed: Option<String>,
    pub aperture: Option<String>,
    pub lens: Option<String>,
    pub comment: Option<String>,
    #[serde_as(as = "Option<tse::date_format::Naive>")]
    pub date: Option<NaiveDateTime>,
    pub gps: Option<(f64, f64)>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ExposureData {
    pub author: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub sspeed: Option<String>,
    pub aperture: Option<String>,
    pub iso: Option<String>,
    pub lens: Option<String>,
    pub description: Option<String>,
    pub comment: Option<String>,
    pub date: Option<NaiveDateTime>,
    pub gps: Option<(f64, f64)>,
}

impl ExposureData {
    pub fn complete(self, other: &Self) -> Self {
        ExposureData {
            author: self.author.or(other.author.clone()),
            make: self.make.or(other.make.clone()),
            model: self.model.or(other.model.clone()),
            sspeed: self.sspeed.or(other.sspeed.clone()),
            aperture: self.aperture.or(other.aperture.clone()),
            iso: self.iso.or(other.iso.clone()),
            lens: self.lens.or(other.lens.clone()),
            description: self.description.or(other.description.clone()),
            comment: self.comment.or(other.comment.clone()),
            date: self.date.or(other.date),
            gps: self.gps.or(other.gps),
        }
    }
}

impl RollData {
    pub fn update(&mut self, change: Self) {
        self.author = change.author.or(self.author.to_owned());
        self.make = change.make.or(self.make.to_owned());
        self.model = change.model.or(self.model.to_owned());
        self.iso = change.iso.or(self.iso.to_owned());
        self.description = change.description.or(self.description.to_owned());
    }
}

impl ExposureSpecificData {
    pub fn update(&mut self, change: Self) {
        self.sspeed = change.sspeed.or(self.sspeed.to_owned());
        self.aperture = change.aperture.or(self.aperture.to_owned());
        self.comment = change.comment.or(self.comment.to_owned());
        self.lens = change.lens.or(self.lens.to_owned());
        self.date = change.date.or(self.date.to_owned());
        self.gps = change.gps.or(self.gps.to_owned());
    }
}

// Implement the `Add` trait for Orientation.
impl Add for Orientation {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        // Cast enums to u8, add them, and wrap around using modulo 4.
        let result = (self as u8 + rhs as u8) % 4;

        // Safety: The result of `val % 4` is guaranteed to be 0, 1, 2, or 3,
        // which are all valid discriminants for the `Orientation` enum.
        unsafe { std::mem::transmute(result) }
    }
}

impl Orientation {
    pub fn rotate(&self, angle: Orientation) -> Self {
        *self + angle
    }
}

impl From<PathBuf> for FileKind {
    fn from(value: PathBuf) -> Self {
        value
            .extension()
            .and_then(|value| {
                if value == "tse" {
                    return Some(Self::Tse);
                }

                ImageFormat::from_extension(value).map(Self::Image)
            })
            .unwrap_or_default()
    }
}
