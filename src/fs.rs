use crate::Aquiesce;
use futures_lite::StreamExt;
use std::path::Path;
use wasm_bindgen::prelude::wasm_bindgen;

pub async fn write_to_fs(path: &Path, reader: web_sys::FileReader) -> Result<(), crate::Error> {
    let photo_data = js_sys::Uint8Array::new(&reader.result()?).to_vec();

    web_fs::write(path, &photo_data).await?;

    Ok(())
}

pub async fn clear_dir<P: AsRef<Path>>(path: P) -> Result<(), crate::Error> {
    let mut dir = web_fs::read_dir(path).await?;
    while let Some(entry) = dir.next().await {
        let entry = entry?;
        if entry.file_type().await?.is_dir() {
            Box::pin(clear_dir(entry.path())).await?;
            web_fs::remove_dir(entry.path()).await?;
        } else {
            web_fs::remove_file(entry.path()).await?;
        }
    }

    Ok(())
}

pub async fn print_dir_recursively<P: AsRef<Path>>(
    path: P,
    level: usize,
    output: &mut impl std::fmt::Write,
) -> Result<(), crate::Error> {
    let mut dir = web_fs::read_dir(path).await?;
    while let Some(entry) = dir.next().await {
        let entry = entry?;
        writeln!(output, "{}{:?}", " ".repeat(level * 4), entry)?;
        if entry.file_type().await?.is_dir() {
            Box::pin(print_dir_recursively(entry.path(), level + 1, output)).await?;
        }
    }

    Ok(())
}

#[wasm_bindgen]
pub async fn print_all() {
    wasm_bindgen_futures::spawn_local(async move {
        let mut fs_log = String::new();
        print_dir_recursively("", 0, &mut fs_log).await.aquiesce();
        log::info!("{}", fs_log);
    });
}

#[wasm_bindgen]
pub fn clear() {
    wasm_bindgen_futures::spawn_local(async move {
        clear_dir("").await.aquiesce();
    });
}
