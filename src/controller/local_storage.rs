use crate::{
    Error, SessionStorageExt,
    helpers::storage,
    models::{Data, Meta, Selection, TseFormat},
};

pub fn clear() -> Result<(), Error> {
    storage()?.clear().map_err(|e| e.into())
}

pub fn get_data() -> Result<Data, Error> {
    serde_json::from_str(&storage()?.get_existing("data")?).map_err(|e| e.into())
}

pub fn set_data(data: &Data) -> Result<(), Error> {
    storage()?
        .set_item("data", &serde_json::to_string(&data)?)
        .map_err(|e| e.into())
}

pub fn get_metadata() -> Result<Meta, Error> {
    serde_json::from_str(&storage()?.get_existing("metadata")?).map_err(|e| e.into())
}

pub fn set_metadata(metadata: &Meta) -> Result<(), Error> {
    storage()?
        .set_item("metadata", &serde_json::to_string(&metadata)?)
        .map_err(|e| e.into())
}

pub fn get_selection() -> Result<Selection, Error> {
    serde_json::from_str(&storage()?.get_existing("selected")?).or(Ok(Selection::default()))
}

pub fn set_selection(selection: &Selection) -> Result<(), Error> {
    storage()?
        .set_item("selected", &serde_json::to_string(&selection)?)
        .map_err(|e| e.into())
}

pub fn get_tse() -> Result<String, Error> {
    get_data().map(|d| TseFormat::as_tse(&d))
}
