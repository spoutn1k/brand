use crate::Error;
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashSet, ops::Add, path::PathBuf};

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct FileMetadata {
    pub name: String,
    pub local_fs_path: PathBuf,
    pub index: u32,
    pub orientation: Orientation,
    pub file_type: FileKind,
}

#[repr(u8)]
#[derive(Default, Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Orientation {
    #[default]
    Normal = 0,
    Rotated90 = 1,
    Rotated180 = 2,
    Rotated270 = 3,
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

pub trait ReorderMetadataExt {
    fn reorder(self, old: u32, new: u32) -> Self;
}

impl ReorderMetadataExt for Vec<FileMetadata> {
    fn reorder(self, old: u32, new: u32) -> Self {
        let (mut singleton, mut rest): (Vec<_>, Vec<_>) =
            self.into_iter().partition(|entry| entry.index == old);

        assert!(singleton.len() == 1);

        let mut reordered = singleton.pop().expect("Selection does not exist");
        reordered.index = new;

        match new.cmp(&old) {
            Ordering::Greater => {
                rest.iter_mut().for_each(|item| {
                    if (old..=new).contains(&item.index) {
                        item.index -= 1;
                    }
                });
            }
            Ordering::Less => {
                rest.iter_mut().for_each(|item| {
                    if (new..old).contains(&item.index) {
                        item.index += 1;
                    }
                });
            }
            Ordering::Equal => (),
        }

        rest.push(reordered);
        rest
    }
}

#[test]
fn test_reorder_metadata() {
    let mut metadata: Vec<FileMetadata>= serde_json::from_str(r#"[{"name":"06.tiff","local_fs_path":"originals/06.tiff","index":6,"orientation":"Normal","file_type":"image/tiff"},{"name":"32.tiff","local_fs_path":"originals/32.tiff","index":32,"orientation":"Normal","file_type":"image/tiff"},{"name":"01.tiff","local_fs_path":"originals/01.tiff","index":1,"orientation":"Normal","file_type":"image/tiff"},{"name":"23.tiff","local_fs_path":"originals/23.tiff","index":23,"orientation":"Normal","file_type":"image/tiff"},{"name":"07.tiff","local_fs_path":"originals/07.tiff","index":7,"orientation":"Normal","file_type":"image/tiff"},{"name":"10.tiff","local_fs_path":"originals/10.tiff","index":10,"orientation":"Normal","file_type":"image/tiff"},{"name":"03.tiff","local_fs_path":"originals/03.tiff","index":3,"orientation":"Normal","file_type":"image/tiff"},{"name":"15.tiff","local_fs_path":"originals/15.tiff","index":15,"orientation":"Normal","file_type":"image/tiff"},{"name":"16.tiff","local_fs_path":"originals/16.tiff","index":16,"orientation":"Normal","file_type":"image/tiff"},{"name":"17.tiff","local_fs_path":"originals/17.tiff","index":17,"orientation":"Normal","file_type":"image/tiff"},{"name":"26.tiff","local_fs_path":"originals/26.tiff","index":26,"orientation":"Normal","file_type":"image/tiff"},{"name":"20.tiff","local_fs_path":"originals/20.tiff","index":20,"orientation":"Normal","file_type":"image/tiff"},{"name":"05.tiff","local_fs_path":"originals/05.tiff","index":5,"orientation":"Normal","file_type":"image/tiff"},{"name":"34.tiff","local_fs_path":"originals/34.tiff","index":34,"orientation":"Normal","file_type":"image/tiff"},{"name":"21.tiff","local_fs_path":"originals/21.tiff","index":21,"orientation":"Normal","file_type":"image/tiff"},{"name":"09.tiff","local_fs_path":"originals/09.tiff","index":9,"orientation":"Normal","file_type":"image/tiff"},{"name":"35.tiff","local_fs_path":"originals/35.tiff","index":35,"orientation":"Normal","file_type":"image/tiff"},{"name":"14.tiff","local_fs_path":"originals/14.tiff","index":14,"orientation":"Normal","file_type":"image/tiff"},{"name":"31.tiff","local_fs_path":"originals/31.tiff","index":31,"orientation":"Normal","file_type":"image/tiff"},{"name":"33.tiff","local_fs_path":"originals/33.tiff","index":33,"orientation":"Normal","file_type":"image/tiff"},{"name":"04.tiff","local_fs_path":"originals/04.tiff","index":4,"orientation":"Normal","file_type":"image/tiff"},{"name":"27.tiff","local_fs_path":"originals/27.tiff","index":27,"orientation":"Normal","file_type":"image/tiff"},{"name":"08.tiff","local_fs_path":"originals/08.tiff","index":8,"orientation":"Normal","file_type":"image/tiff"},{"name":"19.tiff","local_fs_path":"originals/19.tiff","index":19,"orientation":"Normal","file_type":"image/tiff"},{"name":"11.tiff","local_fs_path":"originals/11.tiff","index":11,"orientation":"Normal","file_type":"image/tiff"},{"name":"22.tiff","local_fs_path":"originals/22.tiff","index":22,"orientation":"Normal","file_type":"image/tiff"},{"name":"18.tiff","local_fs_path":"originals/18.tiff","index":18,"orientation":"Normal","file_type":"image/tiff"},{"name":"25.tiff","local_fs_path":"originals/25.tiff","index":25,"orientation":"Normal","file_type":"image/tiff"},{"name":"29.tiff","local_fs_path":"originals/29.tiff","index":29,"orientation":"Normal","file_type":"image/tiff"},{"name":"13.tiff","local_fs_path":"originals/13.tiff","index":13,"orientation":"Normal","file_type":"image/tiff"},{"name":"02.tiff","local_fs_path":"originals/02.tiff","index":2,"orientation":"Normal","file_type":"image/tiff"},{"name":"12.tiff","local_fs_path":"originals/12.tiff","index":12,"orientation":"Normal","file_type":"image/tiff"},{"name":"30.tiff","local_fs_path":"originals/30.tiff","index":30,"orientation":"Normal","file_type":"image/tiff"},{"name":"24.tiff","local_fs_path":"originals/24.tiff","index":24,"orientation":"Normal","file_type":"image/tiff"},{"name":"28.tiff","local_fs_path":"originals/28.tiff","index":28,"orientation":"Normal","file_type":"image/tiff"}]"#).unwrap();

    metadata.sort_by(|a, b| a.index.cmp(&b.index));

    insta::assert_snapshot!(
        metadata
            .clone()
            .reorder(31, 2)
            .into_iter()
            .map(|m| format!("{} => {}\n", m.name, m.index))
            .collect::<String>()
    );
    insta::assert_snapshot!(
        metadata
            .reorder(7, 19)
            .into_iter()
            .map(|m| format!("{} => {}\n", m.name, m.index))
            .collect::<String>()
    );
}
