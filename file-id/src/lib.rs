/// A utility to read file IDs.
///
/// Modern file systems assign a unique ID to each file. On Linux and MacOS it is called an `inode number`, on Windows it is called `file index`.
/// Together with the `device id`, a file can be identified uniquely on a device at a given time.
///
/// Keep in mind though, that IDs may be re-used at some point.
///
/// ## Example
///
/// ```rust
/// # let file = tempfile::NamedTempFile::new().unwrap();
/// # let path = file.path();
///
/// let file_id = file_id::get_file_id(path).unwrap();
///
/// println!("{file_id:?}");
/// ```
use std::{fs, io, path::Path};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier of a file
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FileId {
    /// Device ID or volume serial number
    pub device: u64,

    /// Inode number or file index
    pub file: u64,
}

impl FileId {
    pub fn new(device: u64, file: u64) -> Self {
        Self { device, file }
    }
}

/// Get the `FileId` for the file at `path`
#[cfg(target_family = "unix")]
pub fn get_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(path.as_ref())?;

    Ok(FileId {
        device: metadata.dev(),
        file: metadata.ino(),
    })
}

/// Get the `FileId` for the file at `path`
#[cfg(target_family = "windows")]
pub fn get_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    use winapi_util::{file::information, Handle};

    let handle = Handle::from_path_any(path.as_ref())?;
    let info = information(&handle)?;

    Ok(FileId {
        device: info.volume_serial_number(),
        file: info.file_index(),
    })
}
