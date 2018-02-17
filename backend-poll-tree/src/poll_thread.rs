use backend::prelude::*;
use walkdir::WalkDir;
use id_tree::{Tree, TreeBuilder, InsertBehavior, Node, NodeId};
use filetime::FileTime;

use std::path::{PathBuf, Path};
use std::ffi::{OsString, OsStr};
use std::sync::mpsc::{self, TryRecvError};
use std::io;
use std::fs;
use std::time::{Duration, Instant};
use std::thread;

struct Element {
    name: OsString,
    mtime: u64,
    size: u64,
    #[cfg(unix)] mode: u32,
}

impl Default for Element {
    fn default() -> Element {
        Element {
            name: OsString::new(),
            mtime: 0,
            size: 0,
            #[cfg(unix)] mode: 0,
        }
    }
}

struct Watch {
    path: PathBuf,
    is_dir: bool,
    tree: Tree<Element>,
}

struct Ancestor {
    name: OsString,
    children: Vec<OsString>,
}

pub fn poll_thread(paths: Vec<PathBuf>, interval: Duration, event_tx: mpsc::Sender<io::Result<Event>>, shutdown_rx: mpsc::Receiver<bool>) {
    let shutdown_in_progress = || {
        match shutdown_rx.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => true,
            Err(TryRecvError::Empty) => false
        }
    };

    let mut watches = Vec::with_capacity(paths.len());
    for path in paths {
        match fs::metadata(&path) {
            Ok(metadata) => {
                let watch = Watch {
                    path,
                    is_dir: metadata.is_dir(),
                    tree: TreeBuilder::new()
                        .with_root(Node::new(Element::default()))
                        .build(),
                };
                watches.push(watch);
            }
            Err(err) => event_tx.send(Err(err)).expect("notify: main thread unreachable")
        }
    }

    'main: loop {
        let start = Instant::now();

        if shutdown_in_progress() {
            break 'main;
        }

        for watch in &mut watches {
            if watch.is_dir {
                let mut parent_node_id = watch.tree.root_node_id().unwrap().clone();
                let mut parent_path = watch.path.clone();
                let mut ancestors = vec![
                    Ancestor {
                        name: OsString::new(),
                        children: watch.tree
                            .children(&parent_node_id)
                            .expect("bug in notify: invalid parent node id")
                            .map(|child| child.data().name.clone())
                            .collect(),
                    }
                ];

                for entry in WalkDir::new(&watch.path)
                    .min_depth(1)
                    .follow_links(true)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let path = entry.path();
                    let file_name = path.file_name().unwrap(); // NOTE all paths returned by walkdir have a file name

                    let p = path.parent().unwrap(); // NOTE all paths returned by walkdir have a parent
                    if p != parent_path {
                        // if the new parent is not a child of the previous parent,
                        // walk the tree up to the last common ancestor
                        if !p.starts_with(&parent_path) {
                            while p != parent_path {
                                let directory = ancestors.pop().expect("bug in notify: ancestors is empty");
                                if !directory.children.is_empty() {
                                    // file(s) were removed
                                    // TODO emit events
                                    // TODO remove from tree
                                }
                                parent_path.pop();
                            }
                        }

                        // update current parent
                        parent_node_id = tree_get_element(
                            &watch.tree,
                            p.strip_prefix(&watch.path).unwrap(), // NOTE the parent path is always either equal to the watch path or below the watch path
                        ).expect("bug in notify: child element not found");
                        parent_path = p.to_path_buf();
                        ancestors.push(
                            Ancestor {
                                name: file_name.to_os_string(),
                                children: watch.tree
                                    .children(&parent_node_id)
                                    .expect("bug in notify: invalid parent node id")
                                    .map(|child| child.data().name.clone())
                                    .collect(),
                            }
                        );
                    }

                    let parent = ancestors.last_mut().expect("bug in notify: ancestors is empty");
                    vec_remove_item(&mut parent.children, file_name);

                    match entry.metadata() {
                        Ok(metadata) => {
                            let mtime = FileTime::from_last_modification_time(&metadata)
                                .seconds();
                            let children_ids = watch.tree
                                .children_ids(&parent_node_id)
                                .expect("bug in notify: invalid parent node id")
                                .cloned()
                                .collect::<Vec<_>>();
                            let mut found = false;
                            for child_id in children_ids {
                                let mut node = watch.tree.get_mut(&child_id).unwrap();
                                let mut data = node.data_mut();
                                if data.name == file_name {
                                    if data.size != metadata.len() {
                                        data.size = metadata.len();
                                        let event = Event {
                                            kind: EventKind::Modify(ModifyKind::Data(DataChange::Size)),
                                            paths: vec![path.to_path_buf()],
                                            relid: None,
                                        };
                                        event_tx.send(Ok(event)).expect("notify: main thread unreachable");
                                    }
                                    if data.mtime != mtime {
                                        data.mtime = mtime;
                                        let event = Event {
                                            kind: EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
                                            paths: vec![path.to_path_buf()],
                                            relid: None,
                                        };
                                        event_tx.send(Ok(event)).expect("notify: main thread unreachable");
                                    }
                                    // TODO check for mode changes
                                    found = true;
                                    break;
                                }
                            }
                            if !found {
                                let element = Element {
                                    name: file_name.to_os_string(),
                                    mtime,
                                    size: metadata.len(),
                                    #[cfg(unix)] mode: 0,
                                };
                                watch.tree.insert(
                                    Node::new(element),
                                    InsertBehavior::UnderNode(&parent_node_id),
                                ).expect("bug in notify: invalid parent node id");
                                let event = Event {
                                    kind: EventKind::Create(if metadata.is_dir() {
                                        CreateKind::Folder
                                    } else {
                                        CreateKind::File
                                    }),
                                    paths: vec![path.to_path_buf()],
                                    relid: None,
                                };
                                event_tx.send(Ok(event)).expect("notify: main thread unreachable");
                            }
                        }
                        Err(err) => event_tx.send(Err(err.into())).expect("notify: main thread unreachable"),
                    }

                    if shutdown_in_progress() {
                        break 'main;
                    }
                }
            } else {
                match watch.path.metadata() {
                    Ok(metadata) => {
                        let mtime = FileTime::from_last_modification_time(&metadata)
                            .seconds();
                        let root_node_id = watch.tree.root_node_id().unwrap().clone();
                        let mut node = watch.tree.get_mut(&root_node_id).unwrap();
                        let mut data = node.data_mut();
                        if data.size != metadata.len() {
                            data.size = metadata.len();
                            let event = Event {
                                kind: EventKind::Modify(ModifyKind::Data(DataChange::Size)),
                                paths: vec![watch.path.clone()],
                                relid: None,
                            };
                            event_tx.send(Ok(event)).expect("notify: main thread unreachable");
                        }
                        if data.mtime != mtime {
                            data.mtime = mtime;
                            let event = Event {
                                kind: EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
                                paths: vec![watch.path.clone()],
                                relid: None,
                            };
                            event_tx.send(Ok(event)).expect("notify: main thread unreachable");
                        }
                        // TODO check for mode changes
                    }
                    Err(err) => event_tx.send(Err(err.into())).expect("notify: main thread unreachable"),
                }
            }
        }

        if shutdown_in_progress() {
            break 'main;
        }

        let duration_since_start = Instant::now().duration_since(start);
        if interval > duration_since_start {
            thread::park_timeout(interval - duration_since_start);
        }
    }
}

fn vec_remove_item(vec: &mut Vec<OsString>, item: &OsStr) -> Option<OsString> {
    let pos = match vec.iter().position(|x| *x == *item) {
        Some(x) => x,
        None => return None,
    };
    Some(vec.remove(pos))
}

fn tree_get_element(tree: &Tree<Element>, relative_path: &Path) -> Option<NodeId> {
    _tree_get_element_recursive(tree, tree.root_node_id().unwrap().clone(), relative_path, 0)
}

fn _tree_get_element_recursive(
    tree: &Tree<Element>,
    node_id: NodeId,
    relative_path: &Path,
    level: usize,
) -> Option<NodeId> {
    if let Some(name) = relative_path.iter().nth(level) {
        let node_id = tree
            .children_ids(&node_id)
            .expect("bug in notify: invalid parent node id")
            .find(|node_id| tree.get(&node_id).unwrap().data().name == name)?
            .clone();
        _tree_get_element_recursive(tree, node_id, relative_path, level + 1)
    } else if level == relative_path.iter().count() {
        Some(node_id)
    } else {
        None
    }
}
