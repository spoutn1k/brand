use std::cmp::max;

const HISTORY_SIZE: usize = 10;

#[derive(Debug, Default)]
pub struct History<T>
where
    T: Default,
{
    ring: [T; HISTORY_SIZE],
    tracker: (usize, usize),
}

impl<T: Default> History<T> {
    pub fn record(&mut self, element: T) {
        let (revision, last_valid_revision) = self.tracker;

        let position = (revision + 1) % HISTORY_SIZE;
        let last_valid_revision = max(
            (revision + 1).saturating_sub(HISTORY_SIZE),
            last_valid_revision,
        );

        self.ring[position] = element;

        self.tracker = (revision + 1, last_valid_revision);
    }

    pub fn pop(&mut self) -> Option<T> {
        let (revision, last_valid_revision) = self.tracker;
        let mut element = None;

        if revision > last_valid_revision {
            element = Some(std::mem::take(&mut self.ring[revision % HISTORY_SIZE]));
            self.tracker = (revision.saturating_sub(1), last_valid_revision);
        }

        element
    }

    pub fn undoable(&self) -> bool {
        self.tracker.0 > self.tracker.1
    }
}
