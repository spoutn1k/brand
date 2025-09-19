use chrono::{DateTime, NaiveDateTime};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

mod history;
mod selection;
mod tse;

pub use history::History;
pub use selection::Selection;
pub use tse::{TseFormat, read_tse};

pub static MAX_EXPOSURES: u32 = 80;
pub static HTML_INPUT_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
pub static HTML_INPUT_TIMESTAMP_FORMAT_N: &str = "%Y-%m-%dT%H:%M";

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

#[derive(Serialize, Deserialize)]
pub enum WorkerMessage {
    Process(FileMetadata, Box<ExposureData>),
    GenerateThumbnail(FileMetadata),
}

#[derive(Serialize, Deserialize)]
pub struct WorkerCompressionAnswer(pub u32, pub String);

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

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub enum Orientation {
    #[default]
    Normal,
    Rotated90,
    Rotated180,
    Rotated270,
}

impl Orientation {
    pub fn rotate(&self, angle: Orientation) -> Self {
        match (self, angle) {
            (Orientation::Normal, Orientation::Rotated90) => Orientation::Rotated90,
            (Orientation::Normal, Orientation::Rotated180) => Orientation::Rotated180,
            (Orientation::Normal, Orientation::Rotated270) => Orientation::Rotated270,
            (Orientation::Rotated90, Orientation::Rotated90) => Orientation::Rotated180,
            (Orientation::Rotated90, Orientation::Rotated180) => Orientation::Rotated270,
            (Orientation::Rotated90, Orientation::Rotated270) => Orientation::Normal,
            (Orientation::Rotated180, Orientation::Rotated90) => Orientation::Rotated270,
            (Orientation::Rotated180, Orientation::Rotated180) => Orientation::Normal,
            (Orientation::Rotated180, Orientation::Rotated270) => Orientation::Rotated90,
            (Orientation::Rotated270, Orientation::Rotated90) => Orientation::Normal,
            (Orientation::Rotated270, Orientation::Rotated180) => Orientation::Rotated90,
            (Orientation::Rotated270, Orientation::Rotated270) => Orientation::Rotated180,
            (o, Orientation::Normal) => o.clone(),
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct FileMetadata {
    pub name: String,
    pub local_fs_path: Option<PathBuf>,
    pub index: u32,
    pub orientation: Orientation,
    pub file_type: Option<String>,
}

#[derive(PartialEq, Eq, Default)]
pub enum FileKind {
    Image(ImageFormat),
    Tse,
    #[default]
    Unknown,
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

pub type Meta = HashMap<PathBuf, FileMetadata>;
