#![allow(non_upper_case_globals, dead_code)]
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

use std::sync::mpsc::{channel, Sender, Receiver};
use super::{Error, Event, op, Watcher};
use std::path::{Path, PathBuf};
use libc;


// TODO: add this to fsevent_sys
const kCFStringEncodingUTF8: u32 = 0x08000100;
pub type CFStringEncoding = u32;

const kCFCompareEqualTo: i32     = 0;
pub type CFComparisonResult = i32;

// MacOS uses Case Insensitive path
const kCFCompareCaseInsensitive: u32 = 1;
pub type CFStringCompareFlags = u32;

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
  pub fn FSEventStreamInvalidate(streamRef: fs::FSEventStreamRef);
  pub fn FSEventStreamRelease(streamRef: fs::FSEventStreamRef);
  pub fn FSEventStreamUnscheduleFromRunLoop(streamRef: fs::FSEventStreamRef, runLoop: cf::CFRunLoopRef, runLoopMode: cf::CFStringRef);

  pub fn CFRunLoopStop(rl: cf::CFRunLoopRef);
  pub fn CFShowStr (str: cf::CFStringRef);
  pub fn CFStringGetCStringPtr(theString: cf::CFStringRef, encoding: CFStringEncoding) -> *const libc::c_char;
  pub fn CFStringCompare(theString1: cf::CFStringRef, theString2: cf::CFStringRef, compareOptions: CFStringCompareFlags) -> CFComparisonResult;
  pub fn CFArrayRemoveValueAtIndex(theArray: cf::CFMutableArrayRef, idx: cf::CFIndex);
}

pub const NULL: cf::CFRef = cf::NULL;

unsafe fn str_path_to_cfstring_ref(source: &str) -> cf::CFStringRef {
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
    while count < cf::CFArrayGetCount(imaginary) {
      let component = cf::CFArrayGetValueAtIndex(imaginary, count);
      url = cf::CFURLCreateCopyAppendingPathComponent(cf::kCFAllocatorDefault, placeholder, component, false);
      cf::CFRelease(placeholder);
      placeholder = url;
      count = count + 1;
    }
    cf::CFRelease(imaginary);
  }

  let cf_path = cf::CFURLCopyFileSystemPath(placeholder, cf::kCFURLPOSIXPathStyle);
  cf::CFRelease(placeholder);
  cf_path
}

pub struct FsEventWatcher {
  paths: cf::CFMutableArrayRef,
  since_when: fs::FSEventStreamEventId,
  latency: cf::CFTimeInterval,
  flags: fs::FSEventStreamCreateFlags,
  sender: Sender<Event>,
  runloop: Arc<RwLock<Option<usize>>>,
  context: Option<StreamContextInfo>,
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


pub fn is_api_available() -> Result<(), String> {
  let ma = cf::system_version_major();
  let mi = cf::system_version_minor();

  if ma == 10 && mi < 5 {
    Err("This version of OSX does not support the FSEvent library, cannot proceed".to_string())
  } else {
    Ok(())
  }
}

struct StreamContextInfo {
  sender: Sender<Event>,
  done:  Receiver<()>
}

impl FsEventWatcher {
  #[inline]
  pub fn is_running(&self) -> bool {
    self.runloop.read().unwrap().is_some()
  }

  pub fn stop(&mut self) {
    if !self.is_running() {
      return;
    }

    if let Ok(runloop) = self.runloop.read() {
      if let Some(runloop) = runloop.clone() {
        unsafe {
          let runloop = runloop as *mut libc::c_void;
          CFRunLoopStop(runloop);
        }
      }
    }

    self.runloop = Arc::new(RwLock::new(None));
    if let Some(ref context_info)  = self.context {
      // sync done channel
      match context_info.done.recv() {
        Ok(()) => (),
        Err(_) => panic!("the runloop may not be finished!"),
      }
    }

    self.context = None;
  }

  fn remove_path(&mut self, source: &str) {
    unsafe {
      let cf_path = str_path_to_cfstring_ref(source);

      for idx in 0 .. cf::CFArrayGetCount(self.paths) {
        let item = cf::CFArrayGetValueAtIndex(self.paths, idx);
        if CFStringCompare(item, cf_path, kCFCompareCaseInsensitive) == kCFCompareEqualTo {
          CFArrayRemoveValueAtIndex(self.paths, idx);
        }
      }
    }
  }

  // https://github.com/thibaudgg/rb-fsevent/blob/master/ext/fsevent_watch/main.c
  fn append_path(&mut self, source: &str) {
    unsafe {
      let cf_path = str_path_to_cfstring_ref(source);
      cf::CFArrayAppendValue(self.paths, cf_path);
      cf::CFRelease(cf_path);
    }
  }

  pub fn run(&mut self) -> Result<(), Error> {
    if unsafe { cf::CFArrayGetCount(self.paths) } == 0 {
      return Err(Error::PathNotFound);
    }

    // done channel is used to sync quit status of runloop thread
    let (done_tx, done_rx) = channel();

    let info = StreamContextInfo {
      sender: self.sender.clone(),
      done: done_rx
    };

    self.context = Some(info);

    let stream_context = fs::FSEventStreamContext{
      version: 0,
      info: unsafe { transmute::<_, *mut libc::c_void>(self.context.as_ref()) },
      retain: cf::NULL,
      copy_description: cf::NULL };

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
      let runloop = self.runloop.clone();

      thread::spawn(move || {
        let stream = dummy as *mut libc::c_void;
        // fs::FSEventStreamShow(stream);
        let cur_runloop = cf::CFRunLoopGetCurrent();
        {
          let mut runloop = runloop.write().unwrap();
          *runloop = Some(cur_runloop as *mut libc::c_void as usize);
        }
        fs::FSEventStreamScheduleWithRunLoop(stream,
                                             cur_runloop,
                                             cf::kCFRunLoopDefaultMode);

        fs::FSEventStreamStart(stream);

        // the calling to CFRunLoopRun will be terminated by CFRunLoopStop call in drop()
        cf::CFRunLoopRun();
        fs::FSEventStreamStop(stream);
        FSEventStreamInvalidate(stream);
        FSEventStreamRelease(stream);
        let _d = done_tx.send(()).unwrap();
      });
    }

    Ok(())
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
  let info = unsafe { transmute::<_, *const StreamContextInfo>(info) };

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

      let _s = (*info).sender.send(event).unwrap();
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
        sender: tx,
        runloop: Arc::new(RwLock::new(None)),
        context: None,
      };
    }

    Ok(fsevent)
  }

  fn watch(&mut self, path: &Path) -> Result<(), Error> {
    self.stop();
    self.append_path(&path.to_str().unwrap());
    self.run()
  }

  fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
    self.stop();
    self.remove_path(&path.to_str().unwrap());
    // ignore return error: may be empty path list
    let _ = self.run();
    Ok(())
  }
}

impl Drop for FsEventWatcher {
  fn drop(&mut self) {
    self.stop();
    unsafe {
      cf::CFRelease(self.paths);
    }
  }
}



#[test]
fn test_fsevent_watcher_drop() {
  use super::*;
  let (tx, rx) = channel();

  {
    let mut watcher: RecommendedWatcher = Watcher::new(tx).unwrap();
    watcher.watch(&Path::new("../../")).unwrap();
    thread::sleep_ms(2_000);
    println!("is running -> {}", watcher.is_running());

    thread::sleep_ms(1_000);
    watcher.unwatch(&Path::new("../..")).unwrap();
    println!("is running -> {}", watcher.is_running());
  }

  thread::sleep_ms(1_000);

  // if drop() works, this loop will quit after all Sender freed
  // otherwise will block forever
  for e in rx.iter() {
      println!("debug => {:?} {:?}", e.op.map(|e| e.bits()).unwrap_or(0), e.path);
  }

  println!("in test: {} works", file!());
}
