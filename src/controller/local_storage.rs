use crate::{
    Error, SessionStorageExt,
    error::IntoError,
    helpers::storage,
    models::{Data, FileMetadata, Selection, TseFormat},
};

pub fn clear() -> Result<(), Error> {
    storage()?.clear().error()
}

pub fn get_data() -> Result<Data, Error> {
    serde_json::from_str(&storage()?.get_existing("data")?).error()
}

pub fn set_data(data: &Data) -> Result<(), Error> {
    storage()?
        .set_item("data", &serde_json::to_string(&data)?)
        .error()
}

pub fn get_metadata() -> Result<Vec<FileMetadata>, Error> {
    serde_json::from_str(&storage()?.get_existing("metadata")?).error()
}

pub fn set_metadata(metadata: &[FileMetadata]) -> Result<(), Error> {
    storage()?
        .set_item("metadata", &serde_json::to_string(metadata)?)
        .error()
}

pub fn get_selection() -> Result<Selection, Error> {
    serde_json::from_str(&storage()?.get_existing("selected")?).or(Ok(Selection::default()))
}

pub fn set_selection(selection: &Selection) -> Result<(), Error> {
    storage()?
        .set_item("selected", &serde_json::to_string(&selection)?)
        .error()
}

pub fn get_tse() -> Result<String, Error> {
    get_data().map(|d| TseFormat::as_tse(&d))
}
