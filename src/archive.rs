use crate::{Error, controller::get_tse, helpers::download_buffer};
use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

static ARCHIVE_SIZE: usize = 2 * 1024 * 1024 * 1024 - 1; // 2GiB

fn create() -> tar::Builder<Cursor<Vec<u8>>> {
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

pub async fn builder(
    folder_name: PathBuf,
    mut receiver: mpsc::Receiver<PathBuf>,
    ack: oneshot::Sender<()>,
) -> Result<(), Error> {
    let mut archive_builder = create();

    archive_builder.add_file(get_tse()?.as_bytes(), folder_name.clone().join("index.tse"))?;

    let mut archive_num = 1;
    let mut counter: usize = 0;
    while let Some(entry) = receiver.next().await {
        let file = web_fs::read(&entry).await?;

        if counter + file.len() > ARCHIVE_SIZE {
            download_buffer(
                archive_builder.into_inner()?.into_inner().as_slice(),
                &format!("{}-{archive_num}.tar", folder_name.display()),
                "application/x-tar",
            )?;
            archive_num += 1;
            counter = 0;
            archive_builder = create();
        }
        counter += file.len();

        log::info!(
            "Adding file `{}` to archive ({} bytes, {}MB total)",
            entry.to_string_lossy(),
            file.len(),
            counter / (1024 * 1024)
        );

        archive_builder.add_file(&file, folder_name.clone().join(entry))?;
    }

    download_buffer(
        archive_builder.into_inner()?.into_inner().as_slice(),
        &format!("{}-{archive_num}.tar", folder_name.display()),
        "application/x-tar",
    )
    .inspect(|_| log::info!("Async archive builder done. Goodbye."))?;

    ack.send(()).or(Err(Error::OsChannelSend))
}
