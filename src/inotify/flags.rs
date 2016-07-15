#![allow(dead_code)]

bitflags! {
  pub flags Mask: u32 {
    #[doc = " Event: File was accessed."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur both for the"]
    #[doc = " directory itself and the files within."]
    const IN_ACCESS         = 0x00000001,

    #[doc = "Event: File was modified."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur *only* for"]
    #[doc = " the files within, not the directory itself."]
    const IN_MODIFY         = 0x00000002,

    #[doc = " Event: Metadata has changed."]
    #[doc = " "]
    #[doc = " This can include e.g."]
    #[doc = " - permissions, see [chmod(2)];"]
    #[doc = " - timestamps, see [utimensat(2)];"]
    #[doc = " - extended attributes, see [setxattr(s)];"]
    #[doc = " - link count, see [link(2)] and [unlink(2)];"]
    #[doc = " - user/group, see [chown(2)]."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur both for the"]
    #[doc = " directory itself and the files within."]
    #[doc = " "]
    #[doc = " [chmod(2)]: http://man7.org/linux/man-pages/man2/chmod.2.html"]
    #[doc = " [utimensat(2)]: http://man7.org/linux/man-pages/man2/utimensat.2.html"]
    #[doc = " [setxattr(2)]: http://man7.org/linux/man-pages/man2/utimensat.2.html"]
    #[doc = " [link(2)]: http://man7.org/linux/man-pages/man2/link.2.html"]
    #[doc = " [unlink(2)]: http://man7.org/linux/man-pages/man2/link.2.html"]
    #[doc = " [chown(2)]: http://man7.org/linux/man-pages/man2/link.2.html"]
    const IN_ATTRIB         = 0x00000004,

    #[doc = " Event: File opened for writing was closed."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur both for the"]
    #[doc = " directory itself and the files within."]
    const IN_CLOSE_WRITE    = 0x00000008,

    #[doc = " Event: File not opened for writing was closed."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur both for the"]
    #[doc = " directory itself and the files within."]
    const IN_CLOSE_NOWRITE  = 0x00000010,

    #[doc = " Event: File was opened."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur both for the"]
    #[doc = " directory itself and the files within."]
    const IN_OPEN           = 0x00000020,

    #[doc = " Event: File or directory was moved away."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur *only* for"]
    #[doc = " the files within, not the directory itself."]
    const IN_MOVED_FROM     = 0x00000040,

    #[doc = " Event: File or directory was moved in."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur *only* for"]
    #[doc = " the files within, not the directory itself."]
    const IN_MOVED_TO       = 0x00000080,

    #[doc = " Event: File or directory was created."]
    #[doc = " "]
    #[doc = " This may also include hard links, symlinks, and UNIX sockets."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur *only* for"]
    #[doc = " the files within, not the directory itself."]
    const IN_CREATE         = 0x00000100,

    #[doc = " Event: File or directory was deleted."]
    #[doc = " "]
    #[doc = " This may also include hard links, symlinks, and UNIX sockets."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur *only* for"]
    #[doc = " the files within, not the directory itself."]
    const IN_DELETE         = 0x00000200,

    #[doc = " Event: Watched file or directory was deleted."]
    #[doc = " "]
    #[doc = " This may also occur if the object is moved to another"]
    #[doc = " filesystem, since [mv(1)] in effect copies the file to the"]
    #[doc = " other filesystem and then deletes it from the original."]
    #[doc = " "]
    #[doc = " An IN_IGNORED event will subsequently be generated."]
    #[doc = " "]
    #[doc = " [mv(1)]: http://man7.org/linux/man-pages/man1/mv.1.html"]
    const IN_DELETE_SELF    = 0x00000400,

    #[doc = " Event: Watched file or directory was moved."]
    const IN_MOVE_SELF      = 0x00000800,

    #[doc = " Event: File or directory was moved away or in."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur *only* for"]
    #[doc = " the files within, not the directory itself."]
    const IN_MOVE           = IN_MOVED_FROM.bits
                            | IN_MOVED_TO.bits,

    #[doc = " Event: File opened was closed."]
    #[doc = " "]
    #[doc = " When monitoring a directory, the event may occur both for the"]
    #[doc = " directory itself and the files within."]
    const IN_CLOSE          = IN_CLOSE_WRITE.bits
                            | IN_CLOSE_NOWRITE.bits,

    #[doc = " Event: Any event occured."]
    const IN_ALL_EVENTS     = IN_ACCESS.bits
                            | IN_MODIFY.bits
                            | IN_ATTRIB.bits
                            | IN_CLOSE_WRITE.bits
                            | IN_CLOSE_NOWRITE.bits
                            | IN_OPEN.bits
                            | IN_MOVED_FROM.bits
                            | IN_MOVED_TO.bits
                            | IN_CREATE.bits
                            | IN_DELETE.bits
                            | IN_DELETE_SELF.bits
                            | IN_MOVE_SELF.bits,

    #[doc = " Option: Don't watch children (if self is a directory)."]
    const IN_ONLYDIR       = 0x01000000,

    #[doc = " Option: Don't dereference (if self is a symlink)."]
    const IN_DONT_FOLLOW   = 0x02000000,

    #[doc = " Option: Don't watch unlinked children."]
    #[doc = " "]
    #[doc = " > By default, when watching events on the children of a"]
    #[doc = " > directory, events are generated for children even after"]
    #[doc = " > they have been unlinked from the directory.  This can"]
    #[doc = " > result in large numbers of uninteresting events for some"]
    #[doc = " > applications (e.g., if watching /tmp, in which many"]
    #[doc = " > applications create temporary files whose names are"]
    #[doc = " > immediately unlinked)."]
    #[doc = " > "]
    #[doc = " > IN_EXCL_UNLINK changes this behavior, so that events are"]
    #[doc = " > not generated for children after they have been unlinked"]
    #[doc = " > from the watched directory."]
    const IN_EXCL_UNLINK   = 0x04000000,

    #[doc = " Option: Add events to an existing watch instead of replacing it."]
    #[doc = " "]
    #[doc = " > If a watch instance already exists for the filesystem"]
    #[doc = " > object corresponding to self, add (|) the events to the"]
    #[doc = " > watch mask instead of replacing it."]
    const IN_MASK_ADD      = 0x20000000,

    #[doc = " Option: Listen for one event, then remove the watch."]
    const IN_ONESHOT       = 0x80000000,

    #[doc = " Info: Subject of this event is a directory."]
    const IN_ISDIR         = 0x40000000,

    #[doc = " Info: Filesystem containing self was unmounted."]
    #[doc = " "]
    #[doc = " An IN_IGNORED event will subsequently be generated."]
    const IN_UNMOUNT       = 0x00002000,

    #[doc = " Info: Event queue overflowed."]
    const IN_Q_OVERFLOW    = 0x00004000,

    #[doc = " Info: Watch was removed."]
    #[doc = " "]
    #[doc = " This can occur either as a result of `inotify_rm_watch()`,"]
    #[doc = " or because self was deleted or the containing filesystem"]
    #[doc = " was unmounted, or after an IN_ONESHOT watch is complete."]
    #[doc = " "]
    #[doc = " See the BUGS section of [inotify(7)] for more details."]
    #[doc = " "]
    #[doc = " [inotify(7)]: http://man7.org/linux/man-pages/man7/inotify.7.html"]
    const IN_IGNORED       = 0x00008000,
  }
}
