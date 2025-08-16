use futures_lite::StreamExt;
use std::path::Path;

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
) {
    let mut dir = web_fs::read_dir(path).await.unwrap();
    while let Some(entry) = dir.next().await {
        let entry = entry.unwrap();
        writeln!(output, "{}{:?}", " ".repeat(level * 4), entry).unwrap();
        if entry.file_type().await.unwrap().is_dir() {
            Box::pin(print_dir_recursively(entry.path(), level + 1, output)).await;
        }
    }
}
