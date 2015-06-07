// via https://github.com/octplane/fsevent-rust

use fsevent_sys::core_foundation as cf;
use fsevent_sys::fsevent as fs;
use std::slice;
use std::ffi::CString;
use std::mem::transmute;
use std::slice::from_raw_parts_mut;
use std::str::from_utf8;
use std::ffi::CStr;
use std::convert::AsRef;
use std::thread;
use std::sync::{Arc, RwLock};

use std::sync::mpsc::{Sender};
use super::{Error, Event, op, Watcher};
use std::path::{Path, PathBuf};

use libc;

pub const NULL: cf::CFRef = cf::NULL;

pub struct FsEventWatcher {
  paths: cf::CFMutableArrayRef,
  since_when: fs::FSEventStreamEventId,
  latency: cf::CFTimeInterval,
  flags: fs::FSEventStreamCreateFlags,
  sender: Arc<RwLock<Sender<Event>>>,
  stream: Option<*mut libc::c_void>
}

bitflags! {
  flags StreamFlags: u32 {
    const NONE = 0x00000000,
    const MUST_SCAN_SUB_DIRS = 0x00000001,
    const USER_DROPPED = 0x00000002,
    const KERNEL_DROPPED = 0x00000004,
    const IDS_WRAPPED = 0x00000008,
    const HISTORY_DONE = 0x00000010,
    const ROOT_CHANGED = 0x00000020,
    const MOUNT = 0x00000040,
    const UNMOUNT = 0x00000080,
    const ITEM_CREATED = 0x00000100,
    const ITEM_REMOVED = 0x00000200,
    const ITEM_INODE_META_MOD = 0x00000400,
    const ITEM_RENAMED = 0x00000800,
    const ITEM_MODIFIED = 0x00001000,
    const ITEM_FINDER_INFO_MOD = 0x00002000,
    const ITEM_CHANGE_OWNER = 0x00004000,
    const ITEM_XATTR_MOD = 0x00008000,
    const ITEM_IS_FILE = 0x00010000,
    const ITEM_IS_DIR = 0x00020000,
    const ITEM_IS_SYMLIMK = 0x00040000,
  }
}

fn translate_flags(flags: StreamFlags) -> op::Op {
  let mut ret = op::Op::empty();
  if flags.contains(ITEM_XATTR_MOD) {
    ret.insert(op::CHMOD);
  }
  if flags.contains(ITEM_CREATED) {
    ret.insert(op::CREATE);
  }
  if flags.contains(ITEM_REMOVED) {
    ret.insert(op::REMOVE);
  }
  if flags.contains(ITEM_RENAMED) {
    ret.insert(op::RENAME);
  }
  if flags.contains(ITEM_MODIFIED)  {
    ret.insert(op::WRITE);
  }
  ret
}


pub fn is_api_available() -> (bool, String) {
  let ma = cf::system_version_major();
  let mi = cf::system_version_minor();

  if ma == 10 && mi < 5 {
    return (false, "This version of OSX does not support the FSEvent library, cannot proceed".to_string());
  }
  return (true, "ok".to_string());
}

fn default_stream_context(info: *const FsEventWatcher) -> fs::FSEventStreamContext {
  let ptr = unsafe { transmute((*info).sender.clone()) };
  let stream_context = fs::FSEventStreamContext{
    version: 0,
    info: ptr,
    retain: cf::NULL,
    copy_description: cf::NULL };

  stream_context
}

impl FsEventWatcher {
  // https://github.com/thibaudgg/rb-fsevent/blob/master/ext/fsevent_watch/main.c
  pub fn append_path(&self, source: &str) {
    unsafe {
      let c_path = CString::new(source).unwrap();
      let c_len = libc::strlen(c_path.as_ptr());
      let mut url = cf::CFURLCreateFromFileSystemRepresentation(cf::kCFAllocatorDefault, c_path.as_ptr(), c_len as i64, false);
      let mut placeholder = cf::CFURLCopyAbsoluteURL(url);
      cf::CFRelease(url);

      let imaginary: cf::CFRef = cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks);

      while !cf::CFURLResourceIsReachable(placeholder, cf::kCFAllocatorDefault) {

        let child = cf::CFURLCopyLastPathComponent(placeholder);
        cf::CFArrayInsertValueAtIndex(imaginary, 0, child);
        cf::CFRelease(child);

        url = cf::CFURLCreateCopyDeletingLastPathComponent(cf::kCFAllocatorDefault, placeholder);
        cf::CFRelease(placeholder);
        placeholder = url;
      }

      url = cf::CFURLCreateFileReferenceURL(cf::kCFAllocatorDefault, placeholder, cf::kCFAllocatorDefault);
      cf::CFRelease(placeholder);
      placeholder = cf::CFURLCreateFilePathURL(cf::kCFAllocatorDefault, url, cf::kCFAllocatorDefault);
      cf::CFRelease(url);

      if imaginary != cf::kCFAllocatorDefault {
        let mut count =  0;
        while { count < cf::CFArrayGetCount(imaginary) }
        {
          let component = cf::CFArrayGetValueAtIndex(imaginary, count);
          url = cf::CFURLCreateCopyAppendingPathComponent(cf::kCFAllocatorDefault, placeholder, component, false);
          cf::CFRelease(placeholder);
          placeholder = url;
          count = count + 1;
        }
        cf::CFRelease(imaginary);
      }

      let cf_path = cf::CFURLCopyFileSystemPath(placeholder, cf::kCFURLPOSIXPathStyle);
      cf::CFArrayAppendValue(self.paths, cf_path);
      cf::CFRelease(cf_path);
      cf::CFRelease(placeholder);
    }
  }
  pub fn run(&mut self) {
    let stream_context = default_stream_context(self);
    println!("debug addr in run() => {:?}", self as *const FsEventWatcher as *mut libc::c_void)  ;

    let cb = callback as *mut _;

    unsafe {
      let stream = fs::FSEventStreamCreate(cf::kCFAllocatorDefault,
                                           cb,
                                           &stream_context,
                                           self.paths,
                                           self.since_when,
                                           self.latency,
                                           self.flags);

      let dummy = stream as u64;
      thread::spawn(move || {
        let stream = dummy as *mut libc::c_void;
        fs::FSEventStreamShow(stream);

        fs::FSEventStreamScheduleWithRunLoop(stream,
                                             cf::CFRunLoopGetCurrent(),
                                             cf::kCFRunLoopDefaultMode);

        fs::FSEventStreamStart(stream);

        // self.stream = Some(stream);
        cf::CFRunLoopRun();
        println!("are you ok?");


      });
    }
  }
}

#[allow(unused_variables)]
pub extern "C" fn callback(
  stream_ref: fs::FSEventStreamRef,
  info: *mut libc::c_void,
  num_events: libc::size_t,      // size_t numEvents
  event_paths: *const *const libc::c_char, // void *eventPaths
  event_flags: *mut libc::c_void, // const FSEventStreamEventFlags eventFlags[]
  event_ids: *mut libc::c_void,  // const FSEventStreamEventId eventIds[]
  ) {
  let num = num_events as usize;
  let e_ptr = event_flags as *mut u32;
  let i_ptr = event_ids as *mut u64;
  let sender = unsafe { transmute::<_, Arc<RwLock<Sender<Event>>>>(info) };

  unsafe {

    let paths: &[*const libc::c_char] = transmute(slice::from_raw_parts(event_paths, num));
    let flags = from_raw_parts_mut(e_ptr, num);
    let ids = from_raw_parts_mut(i_ptr, num);

    for p in (0..num) {
      let i = CStr::from_ptr(paths[p]).to_bytes();
      let flag: StreamFlags = StreamFlags::from_bits(flags[p] as u32)
        .expect(format!("Unable to decode StreamFlags: {}", flags[p] as u32).as_ref());

      let path = PathBuf::from(from_utf8(i).ok().expect("Invalid UTF8 string."));
      let event = Event{op: Ok(translate_flags(flag)), path: Some(path)};

      let sender = sender.read().unwrap();
      let _s = sender.send(event).unwrap();
    }
  }
}


impl Watcher for FsEventWatcher {
  fn new(tx: Sender<Event>) -> Result<FsEventWatcher, Error> {
    let fsevent: FsEventWatcher;

    unsafe {
      fsevent = FsEventWatcher {
        paths: cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks),
        since_when: fs::kFSEventStreamEventIdSinceNow,
        latency: 0.1,
        flags: fs::kFSEventStreamCreateFlagFileEvents,
        sender: Arc::new(RwLock::new(tx)),
        stream: None,
      };
    }
    Ok(fsevent)
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    self.append_path(&path.to_str().unwrap());
    self.run();
    Ok(())
  }

  fn unwatch(&mut self, _path: &Path) -> Result<(), Error> {
    Err(Error::NotImplemented)
  }
}

impl Drop for FsEventWatcher {
  fn drop(&mut self) {
    if let Some(stream) = self.stream {
      unsafe {
        fs::FSEventStreamStop(stream);
      }
    }
  }
}
