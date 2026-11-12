//! Utility for reading inode numbers (Unix) and file IDs (Windows).
//!
//! File systems assign an ID to each file. On Unix it is called an `inode number`,
//! on Windows it is called a `file id` or `file index`.
//! Together with the `device id` (Unix) or `volume serial number` (Windows),
//! this identifies a file or directory on a single computer at a given time,
//! subject to filesystem guarantees. In particular, the Windows low resolution
//! file index is not guaranteed to be unique on ReFS.
//!
//! IDs may be reused after a file is deleted, and some file systems can change
//! a file's ID during its lifetime.
//!
//! [`get_file_id`] follows symlinks (and reparse points on Windows), so a symlink reports the ID of its target.
//! Use [`get_file_id_no_follow`] to get the ID of the link itself, or [`get_file_id_from_file`]
//! to get the ID from an open file. The no-follow functions apply to the final path
//! component, symlinks in parent components are still followed.
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
//! ## Example (Windows only)
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
//!
//! ## Example (Without following symlinks, Unix)
//!
//! ```ignore
//! // A symlink has its own ID; following it returns the target's ID.
//! let dir = tempfile::tempdir().unwrap();
//! let target = dir.path().join("target");
//! let link = dir.path().join("link");
//! std::fs::write(&target, b"contents").unwrap();
//! std::os::unix::fs::symlink(&target, &link).unwrap();
//!
//! let target_id = file_id::get_file_id(&target).unwrap();
//! assert_eq!(file_id::get_file_id(&link).unwrap(), target_id);
//! assert_ne!(file_id::get_file_id_no_follow(&link).unwrap(), target_id);
//! ```
//!
//! ## Example (From an open file)
//!
//! ```
//! let file = tempfile::tempfile().unwrap();
//!
//! let file_id = file_id::get_file_id_from_file(&file).unwrap();
//! println!("{file_id:?}");
//! ```
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(any(unix, windows, doc))]
use std::{fs, io, path::Path};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique identifier of a file or directory.
///
/// IDs may be reused or change over time. See the crate documentation for limitations.
/// Values with different variants compare unequal, even if they identify the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FileId {
    /// Device ID and inode number, available on Unix.
    #[cfg_attr(feature = "serde", serde(rename = "inode"))]
    Inode {
        /// Device ID.
        #[cfg_attr(feature = "serde", serde(rename = "device"))]
        device_id: u64,

        /// Inode number.
        #[cfg_attr(feature = "serde", serde(rename = "inode"))]
        inode_number: u64,
    },

    /// Low resolution file ID, available on Windows XP and above.
    ///
    /// Contains a 32-bit volume serial number and a 64-bit file index.
    /// The file index is not guaranteed to be unique on ReFS.
    ///
    /// On Windows, the low resolution variant can be requested explicitly with the [`get_low_res_file_id`] function.
    ///
    /// Details: <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileinformationbyhandle>.
    #[cfg_attr(feature = "serde", serde(rename = "lowres"))]
    LowRes {
        /// Volume serial number.
        #[cfg_attr(feature = "serde", serde(rename = "volume"))]
        volume_serial_number: u32,

        /// File index.
        #[cfg_attr(feature = "serde", serde(rename = "index"))]
        file_index: u64,
    },

    /// High resolution file ID, available on Windows 8 and Windows Server 2012 and later.
    ///
    /// On Windows, the high resolution variant can be requested explicitly with the [`get_high_res_file_id`] function.
    ///
    /// Details: <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex>.
    #[cfg_attr(feature = "serde", serde(rename = "highres"))]
    HighRes {
        /// Volume serial number.
        #[cfg_attr(feature = "serde", serde(rename = "volume"))]
        volume_serial_number: u64,

        /// File ID.
        #[cfg_attr(feature = "serde", serde(rename = "file"))]
        file_id: u128,
    },
}

impl FileId {
    #[must_use]
    pub fn new_inode(device_id: u64, inode_number: u64) -> Self {
        FileId::Inode {
            device_id,
            inode_number,
        }
    }

    #[must_use]
    pub fn new_low_res(volume_serial_number: u32, file_index: u64) -> Self {
        FileId::LowRes {
            volume_serial_number,
            file_index,
        }
    }

    #[must_use]
    pub fn new_high_res(volume_serial_number: u64, file_id: u128) -> Self {
        FileId::HighRes {
            volume_serial_number,
            file_id,
        }
    }
}

impl AsRef<FileId> for FileId {
    fn as_ref(&self) -> &FileId {
        self
    }
}

/// Returns the [`FileId`] of the file or directory at `path`.
///
/// Follows symlinks (and other reparse points on Windows) and returns the target's ID.
/// See [`get_file_id_no_follow`] for the ID of the link itself.
///
/// # Platform-specific behavior
///
/// On Unix, returns the device ID and inode number.
///
/// On Windows, tries the high resolution variant first and falls back to the low
/// resolution variant if that query fails.
#[cfg(any(unix, windows, doc))]
pub fn get_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::metadata(path.as_ref())?;

        Ok(FileId::new_inode(metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        get_file_id_from_file(&open_file(path)?)
    }
}

/// Returns the [`FileId`] of the file or directory at `path`.
///
/// Returns the ID of the final path component without following it if it is a symlink
/// (or reparse point on Windows). Links in parent components are still followed.
/// See [`get_file_id`] for the ID of the target instead.
///
/// # Platform-specific behavior
///
/// On Unix, returns the device ID and inode number.
///
/// On Windows, tries the high resolution variant first and falls back to the low
/// resolution variant if that query fails.
#[cfg(any(unix, windows, doc))]
pub fn get_file_id_no_follow(path: impl AsRef<Path>) -> io::Result<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path.as_ref())?;

        Ok(FileId::new_inode(metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        get_file_id_from_file(&open_file_no_follow(path)?)
    }
}

/// Returns the [`FileId`] of an open file.
///
/// Returns the ID of the file referenced by the handle.
/// Whether links were followed depends on how the caller opened the file.
/// The ID remains available if the file is renamed after opening.
///
/// # Platform-specific behavior
///
/// On Unix, returns the device ID and inode number.
///
/// On Windows, whether reparse points were followed depends on whether the caller
/// opened the file with `FILE_FLAG_OPEN_REPARSE_POINT`. Tries the high resolution
/// variant first and falls back to the low resolution variant if that query fails.
#[cfg(any(unix, windows, doc))]
pub fn get_file_id_from_file(file: &fs::File) -> io::Result<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = file.metadata()?;

        Ok(FileId::new_inode(metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        get_file_info_ex(file).or_else(|_| get_file_info(file))
    }
}

/// Returns the [`FileId::LowRes`] of the file or directory at `path`.
///
/// Available on Windows only.
///
/// Follows symlinks and other reparse points and returns the target's ID.
/// See [`get_low_res_file_id_no_follow`] for the ID of the link itself.
#[cfg(any(windows, doc))]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub fn get_low_res_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    #[cfg(windows)]
    {
        let file = open_file(path)?;

        get_file_info(&file)
    }
}

/// Returns the [`FileId::LowRes`] of the file or directory at `path`.
///
/// Available on Windows only.
///
/// Returns the ID of the final path component without following it if it is a symlink
/// or reparse point. Symlinks and reparse points in parent components are still followed.
#[cfg(any(windows, doc))]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub fn get_low_res_file_id_no_follow(path: impl AsRef<Path>) -> io::Result<FileId> {
    #[cfg(windows)]
    {
        let file = open_file_no_follow(path)?;

        get_file_info(&file)
    }
}

/// Returns the [`FileId::HighRes`] of the file or directory at `path`.
///
/// Available on Windows only.
///
/// Follows symlinks and other reparse points and returns the target's ID.
/// See [`get_high_res_file_id_no_follow`] for the ID of the link itself.
///
/// Returns an error if the high resolution query fails; does not fall back to low resolution.
#[cfg(any(windows, doc))]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub fn get_high_res_file_id(path: impl AsRef<Path>) -> io::Result<FileId> {
    #[cfg(windows)]
    {
        let file = open_file(path)?;

        get_file_info_ex(&file)
    }
}

/// Returns the [`FileId::HighRes`] of the file or directory at `path`.
///
/// Available on Windows only.
///
/// Returns the ID of the final path component without following it if it is a symlink
/// or reparse point. Symlinks and reparse points in parent components are still followed.
///
/// Returns an error if the high resolution query fails; does not fall back to low resolution.
#[cfg(any(windows, doc))]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub fn get_high_res_file_id_no_follow(path: impl AsRef<Path>) -> io::Result<FileId> {
    #[cfg(windows)]
    {
        let file = open_file_no_follow(path)?;

        get_file_info_ex(&file)
    }
}

#[cfg(target_family = "windows")]
fn get_file_info_ex(file: &fs::File) -> Result<FileId, io::Error> {
    use std::{mem::MaybeUninit, os::windows::prelude::*};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let mut info = MaybeUninit::<FILE_ID_INFO>::zeroed();
    // SAFETY: `file` is an open file, so its handle is valid for the duration of
    // the call; `info` is a live, writable buffer of `size_of::<FILE_ID_INFO>()`
    // bytes, matching the `FileIdInfo` information class.
    let ret = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ret == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `info` is initialized by `GetFileInformationByHandleEx`.
    let info = unsafe { info.assume_init() };
    Ok(FileId::new_high_res(
        info.VolumeSerialNumber,
        u128::from_le_bytes(info.FileId.Identifier),
    ))
}

#[cfg(target_family = "windows")]
fn get_file_info(file: &fs::File) -> Result<FileId, io::Error> {
    use std::{mem::MaybeUninit, os::windows::prelude::*};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `file` is an open file, so its handle is valid for the duration of
    // the call; `info` is a live, writable `BY_HANDLE_FILE_INFORMATION` buffer.
    let ret = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
    if ret == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `info` is initialized by `GetFileInformationByHandle`.
    let info = unsafe { info.assume_init() };
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

#[cfg(target_family = "windows")]
fn open_file_no_follow<P: AsRef<Path>>(path: P) -> io::Result<fs::File> {
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
