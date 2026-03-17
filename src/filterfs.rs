use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use fuser::{
    Errno, FileAttr, FileHandle, Filesystem, FopenFlags, Generation, INodeNo, LockOwner,
    OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, Request,
};
use log::{debug, trace};

use crate::fhman::FhManager;
use crate::inoman::INodeManager;
use crate::pattern::PatternRule;
use crate::utils;

const TTL: Duration = Duration::from_secs(1);

pub struct FilterFS {
    // root: PathBuf,
    dir_incl: Vec<PatternRule>,
    dir_excl: Vec<PatternRule>,
    file_incl: Vec<PatternRule>,
    file_excl: Vec<PatternRule>,
    prune_depth: usize,
    inoman: Mutex<INodeManager>,
    fhman: Mutex<FhManager>,
}

impl FilterFS {
    pub fn new(
        root: PathBuf,
        prune_depth: usize,
        file_incl: Vec<PatternRule>,
        file_excl: Vec<PatternRule>,
        dir_incl: Vec<PatternRule>,
        dir_excl: Vec<PatternRule>,
    ) -> Self {
        let inoman = INodeManager::new(&root);
        Self {
            // root,
            dir_incl,
            dir_excl,
            file_incl,
            file_excl,
            prune_depth,
            inoman: Mutex::new(inoman),
            fhman: Mutex::new(FhManager::new()),
        }
    }

    fn is_empty_dir(&self, dir: &Path, depth: usize) -> bool {
        trace!("checking if {:?} is empty", dir);
        // recursion escape
        if depth == 0 {
            trace!("it aint, reached recursion depth");
            return false;
        }
        let rd = if let Ok(rd) = fs::read_dir(dir) {
            rd
        } else {
            trace!("it aint, failed to read dir");
            return false;
        };
        let result = rd
            .into_iter()
            .find(|entry| {
                let entry = if let Ok(entry) = entry {
                    entry
                } else {
                    // no suppression of failed entries
                    return true;
                };
                let path = entry.path();

                if path.is_dir() {
                    self.include_dir(&path) && self.is_empty_dir(&path, depth - 1)
                } else {
                    self.include_file(&path)
                }
            })
            .is_none();
        if result {
            trace!("{:?} is empty!", dir);
        } else {
            trace!("{:?} is not empty!", dir);
        }
        result
    }

    fn include_file(&self, file: &Path) -> bool {
        let mut include = self.file_incl.is_empty();
        for rule in &self.file_incl {
            if rule.include(file) {
                trace!("Include rule: {:?} matches {:?}!", rule, file);
                include = true;
                break;
            }
        }

        if !include {
            return false;
        }

        self.file_excl.iter().all(|rule| rule.include(file))
    }

    fn include_dir(&self, dir: &Path) -> bool {
        let mut include = self.dir_incl.is_empty();
        for rule in &self.dir_incl {
            if rule.include(dir) {
                trace!("Include rule: {:?} matches {:?}!", rule, dir);
                include = true;
                break;
            }
        }

        if !include {
            return false;
        }

        self.dir_excl.iter().all(|rule| rule.include(dir))
    }
}

macro_rules! autorep {
    ($val:expr, $rep:ident, $type:expr) => {
        match $val {
            Some(v) => v,
            None => {
                $rep.error($type);
                return;
            }
        }
    };

    ($val:expr, $rep:ident) => {
        match $val {
            Some(v) => v,
            None => {
                $rep.error(Errno::EIO);
                return;
            }
        }
    };
}

impl Filesystem for FilterFS {
    /// Get file attributes.
    fn getattr(&self, _req: &Request, ino: INodeNo, fh: Option<FileHandle>, reply: ReplyAttr) {
        debug!("Getting attributes for {}, fh: {:?}", ino, fh);
        let inoman = autorep!(self.inoman.lock().ok(), reply, Errno::EIO);
        let path = autorep!(inoman.path(ino), reply);

        let metadata = autorep!(fs::metadata(&path).ok(), reply, Errno::ENOENT);

        let attr: FileAttr = utils::convert_metadata(&metadata, ino);
        reply.attr(&TTL, &attr);
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let mut inoman = autorep!(self.inoman.lock().ok(), reply);
        let mut path = autorep!(inoman.path(parent), reply);
        path.push(name);
        let ino = inoman.ino(&path);
        let metadata = autorep!(fs::metadata(&path).ok(), reply, Errno::ENOENT);
        reply.entry(
            &TTL,
            &utils::convert_metadata(&metadata, ino),
            Generation(0),
        );
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if flags.acc_mode() != OpenAccMode::O_RDONLY {
            reply.error(Errno::EROFS);
            return;
        }

        let inoman = autorep!(self.inoman.lock().ok(), reply);
        let inode = autorep!(inoman.inode(ino), reply);
        let mut fhman = autorep!(self.fhman.lock().ok(), reply);
        let fh = autorep!(fhman.open(inode), reply);

        reply.opened(FileHandle(fh), FopenFlags::empty());
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let mut fhman = autorep!(self.fhman.lock().ok(), reply);
        fhman.release(fh.0);
        reply.ok();
    }

    fn read(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let fhman = autorep!(self.fhman.lock().ok(), reply);

        let handle = autorep!(fhman.handle(fh.0), reply, Errno::EBADF);
        let mut buf = vec![0u8; size as usize];
        let count = autorep!(handle.handle.read_at(&mut buf, offset).ok(), reply);

        reply.data(&buf[..count]);
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        debug!("Entering readdir, fh: {:?}, offset: {}", fh, offset);
        let mut inoman = autorep!(self.inoman.lock().ok(), reply);
        let path = autorep!(inoman.path(ino), reply);

        // check if directory
        if !path.is_dir() {
            reply.error(Errno::ENOTDIR);
            return;
        }

        for (i, entry_result) in autorep!(fs::read_dir(&path).ok(), reply)
            .filter(|entry_result| {
                if let Ok(entry) = entry_result {
                    let path = entry.path();
                    if path.is_dir() {
                        self.include_dir(&path) && !self.is_empty_dir(&path, self.prune_depth)
                    } else {
                        self.include_file(&path)
                    }
                } else {
                    true
                }
            })
            .skip(offset as usize)
            .enumerate()
        {
            let entry = autorep!(entry_result.ok(), reply);
            let path = entry.path();
            let ino = inoman.ino(&path);
            let kind = autorep!(utils::filetype_of_path(&path), reply);
            let name = autorep!(path.file_name(), reply);
            let name = autorep!(name.to_str(), reply);

            // return offset of next entry
            if reply.add(ino, i as u64 + offset + 1, kind, name) {
                // buffer full
                return;
            }
        }

        reply.ok()
    }
}
