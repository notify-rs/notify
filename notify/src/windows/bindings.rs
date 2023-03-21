#![allow(non_camel_case_types, non_snake_case)]

//! We do our own minimal bindings to the win32 API here with only the things we
//! need. This means we can avoid a dependency on the `windows-sys` crate and its
//! version churn, and the `winapi` due to its maintenance status.
//!
//! These APIs will never change, though they may be supplanted in the future.

pub const INFINITE: u32 = 4294967295;
pub const INVALID_HANDLE_VALUE: isize = -1;
pub type HANDLE = isize;

pub type FILE_ACCESS_FLAGS = u32;
pub const FILE_LIST_DIRECTORY: FILE_ACCESS_FLAGS = 1;

pub type FILE_SHARE_MODE = u32;
pub const FILE_SHARE_DELETE: FILE_SHARE_MODE = 4;
pub const FILE_SHARE_READ: FILE_SHARE_MODE = 1;
pub const FILE_SHARE_WRITE: FILE_SHARE_MODE = 2;

pub type BOOL = i32;

#[repr(C)]
pub struct SECURITY_ATTRIBUTES {
    pub nLength: u32,
    pub lpSecurityDescriptor: *mut std::ffi::c_void,
    pub bInheritHandle: BOOL,
}

pub type FILE_CREATION_DISPOSITION = u32;
pub const OPEN_EXISTING: FILE_CREATION_DISPOSITION = 3;

pub type FILE_FLAGS_AND_ATTRIBUTES = u32;
pub const FILE_FLAG_OVERLAPPED: FILE_FLAGS_AND_ATTRIBUTES = 1073741824;
pub const FILE_FLAG_BACKUP_SEMANTICS: FILE_FLAGS_AND_ATTRIBUTES = 33554432;

pub type FILE_NOTIFY_CHANGE = u32;
pub const FILE_NOTIFY_CHANGE_FILE_NAME: FILE_NOTIFY_CHANGE = 1;
pub const FILE_NOTIFY_CHANGE_DIR_NAME: FILE_NOTIFY_CHANGE = 2;
pub const FILE_NOTIFY_CHANGE_ATTRIBUTES: FILE_NOTIFY_CHANGE = 4;
pub const FILE_NOTIFY_CHANGE_SIZE: FILE_NOTIFY_CHANGE = 8;
pub const FILE_NOTIFY_CHANGE_LAST_WRITE: FILE_NOTIFY_CHANGE = 16;
pub const FILE_NOTIFY_CHANGE_CREATION: FILE_NOTIFY_CHANGE = 64;
pub const FILE_NOTIFY_CHANGE_SECURITY: FILE_NOTIFY_CHANGE = 256;

#[repr(C)]
pub struct OVERLAPPED_0_0 {
    pub Offset: u32,
    pub OffsetHigh: u32,
}
#[repr(C)]
pub union OVERLAPPED_0 {
    pub Anonymous: std::mem::ManuallyDrop<OVERLAPPED_0_0>,
    pub Pointer: *mut std::ffi::c_void,
}
#[repr(C)]
pub struct OVERLAPPED {
    pub Internal: usize,
    pub InternalHigh: usize,
    pub Anonymous: OVERLAPPED_0,
    pub hEvent: HANDLE,
}
pub type LPOVERLAPPED_COMPLETION_ROUTINE = Option<
    unsafe extern "system" fn(
        dwErrorCode: u32,
        dwNumberOfBytesTransfered: u32,
        lpOverlapped: *mut OVERLAPPED,
    ),
>;

pub type FILE_ACTION = u32;
pub const FILE_ACTION_ADDED: FILE_ACTION = 1;
pub const FILE_ACTION_REMOVED: FILE_ACTION = 2;
pub const FILE_ACTION_MODIFIED: FILE_ACTION = 3;
pub const FILE_ACTION_RENAMED_OLD_NAME: FILE_ACTION = 4;
pub const FILE_ACTION_RENAMED_NEW_NAME: FILE_ACTION = 5;

#[repr(C)]
pub struct FILE_NOTIFY_INFORMATION {
    pub NextEntryOffset: u32,
    pub Action: FILE_ACTION,
    pub FileNameLength: u32,
    pub FileName: [u16; 1],
}

pub type WIN32_ERROR = u32;
pub const WAIT_OBJECT_0: WIN32_ERROR = 0;
pub const ERROR_OPERATION_ABORTED: WIN32_ERROR = 995;

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: FILE_ACCESS_FLAGS,
        dwShareMode: FILE_SHARE_MODE,
        lpSecurityAttributes: *const SECURITY_ATTRIBUTES,
        dwCreationDisposition: FILE_CREATION_DISPOSITION,
        dwFlagsAndAttributes: FILE_FLAGS_AND_ATTRIBUTES,
        hTemplateFile: HANDLE,
    ) -> HANDLE;
    pub fn ReadDirectoryChangesW(
        hDirectory: HANDLE,
        lpBuffer: *mut std::ffi::c_void,
        nBufferLength: u32,
        bWatchSubtree: BOOL,
        dwNotifyFilter: FILE_NOTIFY_CHANGE,
        lpBytesReturned: *mut u32,
        lpOverlapped: *mut OVERLAPPED,
        lpCompletionRoutine: LPOVERLAPPED_COMPLETION_ROUTINE,
    ) -> BOOL;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn CancelIo(hFile: HANDLE) -> BOOL;
    pub fn CreateSemaphoreW(
        lpSemaphoreAttributes: *const SECURITY_ATTRIBUTES,
        lInitialCount: i32,
        lMaximumCount: i32,
        lpName: *const u16,
    ) -> HANDLE;
    pub fn ReleaseSemaphore(
        hSemaphore: HANDLE,
        lReleaseCount: i32,
        lpPreviousCount: *mut i32,
    ) -> BOOL;
    pub fn WaitForSingleObjectEx(
        hHandle: HANDLE,
        dwMilliseconds: u32,
        bAlertable: BOOL,
    ) -> WIN32_ERROR;
}
