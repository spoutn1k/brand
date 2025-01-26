use crate::controller::{ExposureUpdate, RollUpdate};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub static MAX_EXPOSURES: u32 = 80;

mod tse_date_format {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    pub struct Naive;

    const FORMAT: &'static str = "%Y %m %d %H %M %S";

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
            Ok(NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?)
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

impl RollData {
    pub fn update(&mut self, change: RollUpdate) {
        match change {
            RollUpdate::Author(value) => self.author = value,
            RollUpdate::Film(value) => self.description = value,
            RollUpdate::ISO(value) => self.iso = value,
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
            self.description.clone().unwrap_or(String::new()),
            self.description.clone().unwrap_or(String::new()),
            self.author.clone().unwrap_or(String::new()),
            self.author.clone().unwrap_or(String::new()),
            self.iso.clone().unwrap_or(String::new()),
            self.make.clone().unwrap_or(String::new()),
            self.model.clone().unwrap_or(String::new()),
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
            ExposureUpdate::GPS(value) => self.gps = value,
        }
    }

    fn as_tsv(&self) -> String {
        let mut fields = vec![
            self.sspeed.clone().unwrap_or(String::new()),
            self.aperture.clone().unwrap_or(String::new()),
            self.lens.clone().unwrap_or(String::new()),
            self.comment.clone().unwrap_or(String::new()),
        ];

        fields.push(
            self.date
                .map(|d| format!("{}", d.format("%Y %m %d %H %M %S")))
                .unwrap_or(String::new()),
        );

        match self.gps {
            None => fields.push(String::new()),
            Some((lat, lon)) => fields.push(format!("{lat}, {lon}")),
        }

        fields.join("\t")
    }
}

impl Data {
    pub fn to_string(&self) -> String {
        let mut lines: Vec<String> = vec![];
        let max: u32 = *self.exposures.keys().max().unwrap_or(&0u32) + 1;

        for index in 1..max {
            match self.exposures.get(&index) {
                Some(exp) => lines.push(exp.as_tsv()),
                None => lines.push(String::new()),
            }
        }

        lines.push(self.roll.as_tsv());
        lines.join("\n")
    }
}

use std::ops::Range;

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Selection {
    last: Option<u32>,
    items: Vec<Range<u32>>,
}

struct Folder {
    ranges: Vec<Range<u32>>,
    current: Range<u32>,
}

impl Folder {
    fn new(start: u32) -> Self {
        Folder {
            ranges: vec![],
            current: start..start + 1,
        }
    }

    fn add(mut self, item: u32) -> Self {
        if item == self.current.end {
            self.current.end = item + 1
        } else {
            self.ranges.push(self.current);
            self.current = item..item + 1;
        }

        self
    }

    fn fin(mut self) -> Vec<Range<u32>> {
        let mut fin = std::mem::take(&mut self.ranges);
        fin.push(self.current);
        fin
    }
}

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
        self.items
            .iter()
            .flat_map(|r| r.clone().into_iter())
            .collect()
    }

    fn add(&mut self, item: u32) {
        self.add_all(item..item + 1)
    }

    fn add_all(&mut self, items: Range<u32>) {
        let mut choices: Vec<u32> = items
            .chain(self.items())
            .collect::<std::collections::HashSet<u32>>()
            .into_iter()
            .collect();

        choices.sort();

        self.items = choices
            .iter()
            .skip(1)
            .fold(Folder::new(choices[0]), |acc, i| acc.add(*i))
            .fin();
    }

    fn del(&mut self, item: u32) {
        let mut choices = self.items();

        choices.retain(|i| *i != item);

        if choices.len() == 0 {
            self.items = vec![];
        } else {
            self.items = choices
                .iter()
                .skip(1)
                .fold(Folder::new(choices[0]), |acc, i| acc.add(*i))
                .fin();
        }
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
            let (min, max) = (std::cmp::min(anchor, item), std::cmp::max(anchor, item));
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
        let choices = self.items();
        let mut all: Vec<u32> = (0..Self::LIMIT).collect();

        all.retain(|i| !choices.contains(i));

        if all.len() == 0 {
            self.items = vec![];
        } else {
            self.items = all
                .iter()
                .skip(1)
                .fold(Folder::new(all[0]), |acc, i| acc.add(*i))
                .fin();
        }
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
        .skip(1)
        .fold(Folder::new(choices[0]), |acc, i| acc.add(*i))
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
