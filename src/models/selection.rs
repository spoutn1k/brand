use serde::{Deserialize, Serialize};
use std::{
    cmp,
    collections::HashSet,
    fmt::{self, Display, Formatter},
    mem,
    ops::Range,
};

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

impl FromIterator<u32> for Selection {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = u32>,
    {
        let mut items: Vec<_> = iter.into_iter().collect();
        items.sort();

        let items = items
            .into_iter()
            .fold(Folder::new(), |acc, index| acc.add(index))
            .fin();

        Selection { last: None, items }
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

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

trait RangeDisplayExt {
    fn to_string(&self) -> String;
}

impl RangeDisplayExt for Range<u32> {
    fn to_string(&self) -> String {
        if self.start == self.end - 1 {
            return self.start.to_string();
        }

        format!("{} - {}", self.start, self.end - 1)
    }
}

impl Display for Selection {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut iter = self.items.iter();

        if let Some(r) = iter.next() {
            write!(f, "{}", r.to_string())?
        }

        while let Some(r) = iter.next() {
            write!(f, ", {}", r.to_string())?
        }

        Ok(())
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
