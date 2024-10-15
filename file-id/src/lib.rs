//! Utility for reading inode numbers (Linux, macOS) and file ids (Windows) that uniquely identify a file on a single computer.
//!
//! Modern file systems assign a unique ID to each file. On Linux and macOS it is called an `inode number`,
//! on Windows it is called a `file id` or `file index`.
//! Together with the `device id` (Linux, macOS) or the `volume serial number` (Windows),
//! a file or directory can be uniquely identified on a single computer at a given time.
//!
//! Keep in mind though, that IDs may be re-used at some point.
//!
//! ## Example
//!
//! ```
//! let file = tempfile::NamedTempFile::new().unwrap();
//!
//! let file_id = file_id::get_file_id(file.path()).unwrap();
//! println!("{file_id:?}");
//! ```
//!
//! ## Example (Windows Only)
//!
//! ```ignore
//! let file = tempfile::NamedTempFile::new().unwrap();
//!
//! let file_id = file_id::get_low_res_file_id(file.path()).unwrap();
//! println!("{file_id:?}");
//!
//! let file_id = file_id::get_high_res_file_id(file.path()).unwrap();
//! println!("{file_id:?}");
//! ```
use std::{fs, io, path::Path};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier of a file
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FileId {
    /// Inode number, available on Linux and macOS.
    #[cfg_attr(feature = "serde", serde(rename = "inode"))]
    Inode {
        /// Device ID
        #[cfg_attr(feature = "serde", serde(rename = "device"))]
        device_id: u64,

        /// Inode number
        #[cfg_attr(feature = "serde", serde(rename = "inode"))]
        inode_number: u64,
    },

    /// Low resolution file ID, available on Windows XP and above.
    ///
    /// Compared to the high resolution variant, only the lower parts of the IDs are stored.
    ///
    /// On Windows, the low resolution variant can be requested explicitly with the `get_low_res_file_id` function.
    ///
    /// Details: <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileinformationbyhandle>.
    #[cfg_attr(feature = "serde", serde(rename = "lowres"))]
    LowRes {
        /// Volume serial number
        #[cfg_attr(feature = "serde", serde(rename = "volume"))]
        volume_serial_number: u32,

        /// File index
        #[cfg_attr(feature = "serde", serde(rename = "index"))]
        file_index: u64,
    },

    /// High resolution file ID, available on Windows Vista and above.
    ///
    /// On Windows, the high resolution variant can be requested explicitly with the `get_high_res_file_id` function.
    ///
    /// Details: <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex>.
    #[cfg_attr(feature = "serde", serde(rename = "highres"))]
    HighRes {
        /// Volume serial number
        #[cfg_attr(feature = "serde", serde(rename = "volume"))]
        volume_serial_number: u64,

        /// File ID
        #[cfg_attr(feature = "serde", serde(rename = "file"))]
        file_id: u128,
    },
}

impl FileId {
    pub fn new_inode(device_id: u64, inode_number: u64) -> Self {
        FileId::Inode {
            device_id,
            inode_number,
        }
    }

    pub fn new_low_res(volume_serial_number: u32, file_index: u64) -> Self {
        FileId::LowRes {
            volume_serial_number,
            file_index,
        }
    }

    pub fn new_high_res(volume_serial_number: u64, file_id: u128) -> Self {
        FileId::HighRes {
            volume_serial_number,
            file_id,
        }
    }
}

/// Get the `FileId` for the file or directory at `path`
#[cfg(target_family = "unix")]
pub fn get_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(path.as_ref())?;

    Ok(FileId::new_inode(metadata.dev(), metadata.ino()))
}

/// Get the `FileId` for the file or directory at `path`
#[cfg(target_family = "windows")]
pub fn get_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    let file = open_file(path)?;

    unsafe { get_file_info_ex(&file).or_else(|_| get_file_info(&file)) }
}

/// Get the `FileId` with the low resolution variant for the file or directory at `path`
#[cfg(target_family = "windows")]
pub fn get_low_res_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    let file = open_file(path)?;

    unsafe { get_file_info(&file) }
}

/// Get the `FileId` with the high resolution variant for the file or directory at `path`
#[cfg(target_family = "windows")]
pub fn get_high_res_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    let file = open_file(path)?;

    unsafe { get_file_info_ex(&file) }
}

#[cfg(target_family = "windows")]
unsafe fn get_file_info_ex(file: &fs::File) -> Result<FileId, io::Error> {
    use std::{mem, os::windows::prelude::*};
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO},
    };

    let mut info: FILE_ID_INFO = mem::zeroed();
    let ret = GetFileInformationByHandleEx(
        file.as_raw_handle() as HANDLE,
        FileIdInfo,
        &mut info as *mut FILE_ID_INFO as _,
        mem::size_of::<FILE_ID_INFO>() as u32,
    );

    if ret == 0 {
        return Err(io::Error::last_os_error());
    };

    Ok(FileId::new_high_res(
        info.VolumeSerialNumber,
        u128::from_le_bytes(info.FileId.Identifier),
    ))
}

#[cfg(target_family = "windows")]
unsafe fn get_file_info(file: &fs::File) -> Result<FileId, io::Error> {
    use std::{mem, os::windows::prelude::*};
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
    };

    let mut info: BY_HANDLE_FILE_INFORMATION = mem::zeroed();
    let ret = GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info);
    if ret == 0 {
        return Err(io::Error::last_os_error());
    };

    Ok(FileId::new_low_res(
        info.dwVolumeSerialNumber,
        ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64),
    ))
}

#[cfg(target_family = "windows")]
fn open_file<P: AsRef<Path>>(path: P) -> io::Result<fs::File> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

    OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}
