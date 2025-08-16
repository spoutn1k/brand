use crate::controller::{ExposureUpdate, RollUpdate};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::{
    cmp,
    collections::{HashMap, HashSet},
    fmt::{self, Display, Formatter},
    mem,
    ops::Range,
};

pub static MAX_EXPOSURES: u32 = 80;

mod tse_date_format {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    pub struct Naive;

    const FORMAT: &str = "%Y %m %d %H %M %S";

    impl SerializeAs<NaiveDateTime> for Naive {
        fn serialize_as<S>(value: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let s = format!("{}", value.format(FORMAT));
            serializer.serialize_str(&s)
        }
    }

    impl<'de> DeserializeAs<'de, NaiveDateTime> for Naive {
        fn deserialize_as<D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Data {
    pub roll: RollData,
    pub exposures: HashMap<u32, ExposureSpecificData>,
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
    #[serde_as(as = "Option<tse_date_format::Naive>")]
    pub date: Option<NaiveDateTime>,
    pub gps: Option<(f64, f64)>,
}

#[derive(Clone, Default, Debug)]
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
    pub fn update(&mut self, change: RollUpdate) {
        match change {
            RollUpdate::Author(value) => self.author = value,
            RollUpdate::Film(value) => self.description = value,
            RollUpdate::Iso(value) => self.iso = value,
            RollUpdate::Make(value) => self.make = value,
            RollUpdate::Model(value) => self.model = value,
        }
    }

    pub fn as_tsv(&self) -> String {
        format!(
            "#Description {}
#ImageDescription {}
#Artist {}
#Author {}
#ISO {}
#Make {}
#Model {}
; vim: set list number noexpandtab:",
            self.description.clone().unwrap_or_default(),
            self.description.clone().unwrap_or_default(),
            self.author.clone().unwrap_or_default(),
            self.author.clone().unwrap_or_default(),
            self.iso.clone().unwrap_or_default(),
            self.make.clone().unwrap_or_default(),
            self.model.clone().unwrap_or_default(),
        )
    }
}

impl ExposureSpecificData {
    pub fn update(&mut self, change: ExposureUpdate) {
        match change {
            ExposureUpdate::ShutterSpeed(value) => self.sspeed = value,
            ExposureUpdate::Aperture(value) => self.aperture = value,
            ExposureUpdate::Comment(value) => self.comment = value,
            ExposureUpdate::Lens(value) => self.lens = value,
            ExposureUpdate::Date(value) => self.date = value,
            ExposureUpdate::Gps(value) => self.gps = value,
        }
    }

    fn as_tsv(&self) -> String {
        let mut fields = vec![
            self.sspeed.clone().unwrap_or_default(),
            self.aperture.clone().unwrap_or_default(),
            self.lens.clone().unwrap_or_default(),
            self.comment.clone().unwrap_or_default(),
        ];

        fields.push(
            self.date
                .map(|d| format!("{}", d.format("%Y %m %d %H %M %S")))
                .unwrap_or_default(),
        );

        match self.gps {
            None => fields.push(String::new()),
            Some((lat, lon)) => fields.push(format!("{lat}, {lon}")),
        }

        fields.join("\t")
    }
}

impl Display for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut lines: Vec<String> = vec![];
        let max: u32 = *self.exposures.keys().max().unwrap_or(&0u32) + 1;

        for index in 1..max {
            match self.exposures.get(&index) {
                Some(exp) => lines.push(exp.as_tsv()),
                None => lines.push(String::new()),
            }
        }

        lines.push(self.roll.as_tsv());
        write!(f, "{}", lines.join("\n"))
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Selection {
    last: Option<u32>,
    items: Vec<Range<u32>>,
}

struct Folder {
    ranges: Vec<Range<u32>>,
    current: Option<Range<u32>>,
}

impl Folder {
    fn new() -> Self {
        Folder {
            ranges: vec![],
            current: None,
        }
    }

    fn add(mut self, item: u32) -> Self {
        match &mut self.current {
            None => self.current = Some(item..item + 1),
            Some(range) if item == range.end => range.end = item + 1,
            Some(range) => {
                self.ranges.push(range.to_owned());
                self.current = Some(item..item + 1);
            }
        }

        self
    }

    fn fin(mut self) -> Vec<Range<u32>> {
        let mut fin = mem::take(&mut self.ranges);
        if let Some(range) = self.current {
            fin.push(range);
        }
        fin
    }
}

#[allow(clippy::single_range_in_vec_init)]
impl Selection {
    const LIMIT: u32 = 256;

    pub fn contains(&self, index: u32) -> bool {
        self.items.iter().any(|r| r.contains(&index))
    }

    pub fn set_one(&mut self, index: u32) {
        self.last = Some(index);
        self.items = vec![index..index + 1]
    }

    pub fn items(&self) -> Vec<u32> {
        self.items.iter().flat_map(|r| r.clone()).collect()
    }

    fn add(&mut self, item: u32) {
        self.add_all(item..item + 1)
    }

    fn add_all(&mut self, items: Range<u32>) {
        let mut choices: Vec<u32> = items
            .chain(self.items())
            .collect::<HashSet<u32>>()
            .into_iter()
            .collect();

        choices.sort();

        self.items = choices
            .into_iter()
            .fold(Folder::new(), |acc, i| acc.add(i))
            .fin();
    }

    fn del(&mut self, item: u32) {
        self.items = self
            .items()
            .into_iter()
            .filter(|i| *i != item)
            .fold(Folder::new(), |acc, i| acc.add(i))
            .fin();
    }

    pub fn toggle(&mut self, item: u32) {
        if self.contains(item) {
            self.del(item)
        } else {
            self.add(item)
        }
    }

    pub fn group_select(&mut self, item: u32) {
        if let Some(anchor) = self.last {
            let (min, max) = (cmp::min(anchor, item), cmp::max(anchor, item));
            self.add_all(min..max + 1)
        }
    }

    pub fn clear(&mut self) {
        self.items = vec![];
    }

    pub fn all(&mut self) {
        self.items = vec![0..Self::LIMIT];
    }

    pub fn invert(&mut self) {
        self.items = (0..Self::LIMIT)
            .filter(|i| !self.contains(*i))
            .fold(Folder::new(), |acc, i| acc.add(i))
            .fin();
    }
}

#[test]
fn test_selection_contains() {
    let mut selection = Selection::default();

    selection.set_one(5);
    assert_eq!(selection.contains(5), true);
    assert_eq!(selection.contains(4), false);
}

#[test]
fn test_sorted_vec_to_select() {
    let choices = [1, 2, 3, 7, 9, 10];

    let selection = choices
        .iter()
        .fold(Folder::new(), |acc, i| acc.add(*i))
        .fin();

    assert_eq!(selection, vec![1..4, 7..8, 9..11])
}

#[test]
fn test_selection_add() {
    let mut sel = Selection {
        last: None,
        items: vec![1..4, 5..7],
    };

    sel.add(4);
    assert_eq!(sel.items, vec![1..7]);

    sel.add(10);
    assert_eq!(sel.items, vec![1..7, 10..11])
}

#[test]
fn test_selection_del() {
    let mut sel = Selection {
        last: None,
        items: vec![1..7],
    };

    sel.del(4);
    assert_eq!(sel.items, vec![1..4, 5..7]);

    sel.del(1);
    assert_eq!(sel.items, vec![2..4, 5..7]);

    sel.del(6);
    assert_eq!(sel.items, vec![2..4, 5..6]);
}
