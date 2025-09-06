use crate::{Error, SessionStorageExt, download_buffer, models::Data, storage};
use futures::StreamExt;
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

static ARCHIVE_SIZE: usize = 2 * 1024 * 1024 * 1024 - 1; // 2GiB

fn create_archive() -> tar::Builder<Cursor<Vec<u8>>> {
    tar::Builder::new(Cursor::new(Vec::<u8>::with_capacity(ARCHIVE_SIZE)))
}

trait AddFileExt {
    fn add_file<S: AsRef<Path>>(&mut self, file: &[u8], path: S) -> Result<(), Error>;
}

impl<W: Write> AddFileExt for tar::Builder<W> {
    fn add_file<S: AsRef<Path>>(&mut self, file: &[u8], path: S) -> Result<(), Error> {
        let secs = instant::SystemTime::now()
            .duration_since(instant::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut header = tar::Header::new_gnu();
        header.set_size(file.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        header.set_mtime(secs);
        if let Some(h) = header.as_gnu_mut() {
            h.set_atime(secs);
            h.set_ctime(secs);
        }

        Ok(self.append_data(&mut header, path.as_ref(), Cursor::new(file))?)
    }
}

pub async fn export_dir<P: AsRef<Path>>(path: P, folder_name: PathBuf) -> Result<(), Error> {
    let mut archive_builder = create_archive();

    let data: Data = serde_json::from_str(&storage()?.get_existing("data")?)?;
    let file = data.to_string();

    archive_builder.add_file(file.as_bytes(), folder_name.clone().join("index.tse"))?;

    let mut archive_num = 1;
    let mut counter: usize = 0;
    let mut stream = web_fs::read_dir(path).await?;
    while let Some(entry) = stream.next().await {
        let entry = entry?;

        if entry.file_type().await?.is_dir() {
            continue;
        }

        let file = web_fs::read(entry.path()).await?;

        if counter + file.len() > ARCHIVE_SIZE {
            download_buffer(
                archive_builder.into_inner()?.into_inner().as_slice(),
                &format!("{}-{archive_num}.tar", folder_name.display()),
                "application/x-tar",
            )?;
            archive_num += 1;
            counter = 0;
            archive_builder = create_archive();
        }
        counter += file.len();

        log::info!(
            "Adding file {} to archive ({} bytes, {}MB total)",
            entry.path().display(),
            file.len(),
            counter / (1024 * 1024)
        );

        archive_builder.add_file(
            &file,
            folder_name.clone().join(
                entry
                    .path()
                    .file_name()
                    .ok_or(Error::MissingKey("Entry path has no file name".into()))?,
            ),
        )?;
    }

    download_buffer(
        archive_builder.into_inner()?.into_inner().as_slice(),
        &format!("{}-{archive_num}.tar", folder_name.display()),
        "application/x-tar",
    )
}
