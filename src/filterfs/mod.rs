use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use fuser::{
    Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo, LockOwner,
    OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, Request,
};
use log::{debug, trace};

use crate::fhman::{FhId, FhManager};
use crate::inoman::INodeManager;
use crate::pattern::PatternRule;
use crate::utils;

#[cfg(test)]
mod tests;

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
        root: &Path,
        prune_depth: usize,
        file_incl: Vec<PatternRule>,
        file_excl: Vec<PatternRule>,
        dir_incl: Vec<PatternRule>,
        dir_excl: Vec<PatternRule>,
    ) -> Self {
        let inoman = INodeManager::new(root);
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

    fn is_empty_dir(&self, dir: &Path, depth: isize) -> bool {
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
                    self.include_dir(&path) && !self.is_empty_dir(&path, depth - 1)
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

    fn getattr(&self, ino: INodeNo, fh: Option<FhId>) -> Result<FileAttr, Errno> {
        let metadata = if let Some(fh) = fh {
            let fhman = self.fhman.lock().map_err(|_| Errno::EIO)?;
            let handle = &fhman.handle(fh).ok_or(Errno::EIO)?.handle;

            handle.metadata().map_err(|_| Errno::ENOENT)?
        } else {
            let inoman = self.inoman.lock().map_err(|_| Errno::EIO)?;
            let path = inoman.path(ino).ok_or(Errno::EIO)?;

            fs::metadata(&path)?
        };

        Ok(utils::convert_metadata(&metadata, ino))
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr) -> Result<FileAttr, Errno> {
        let mut inoman = self.inoman.lock().map_err(|_| Errno::EIO)?;
        let path = inoman.path(parent).ok_or(Errno::ENOENT)?.join(name);

        if !self.include_file(&path) {
            return Err(Errno::ENOENT);
        }

        let ino = inoman.ino(&path);
        let metadata = fs::metadata(&path)?;

        Ok(utils::convert_metadata(&metadata, ino))
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags) -> Result<FileHandle, Errno> {
        if flags.acc_mode() != OpenAccMode::O_RDONLY {
            return Err(Errno::EROFS);
        }

        let inoman = self.inoman.lock().map_err(|_| Errno::EIO)?;
        let inode = inoman.inode(ino).ok_or(Errno::ENOENT)?;
        let mut fhman = self.fhman.lock().map_err(|_| Errno::EIO)?;

        Ok(FileHandle(fhman.open(inode)?))
    }

    fn read(
        &self,
        _req: &Request,
        fh: FhId,
        offset: u64,
        size: usize,
        _flags: OpenFlags,
    ) -> Result<Vec<u8>, Errno> {
        let fhman = self.fhman.lock().map_err(|_| Errno::EIO)?;
        let handle = fhman.handle(fh).ok_or(Errno::EBADF)?;
        let mut buf = vec![0u8; size];
        let count = handle.handle.read_at(&mut buf, offset)?;

        buf.truncate(count);

        Ok(buf)
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FhId,
        offset: u64,
        reply: &mut ReplyDirectory,
    ) -> Result<(), Errno> {
        let mut inoman = self.inoman.lock().map_err(|_| Errno::EIO)?;
        let path = inoman.path(ino).ok_or(Errno::ENOENT)?;

        // check if dir
        if !path.is_dir() {
            return Err(Errno::ENOTDIR);
        }

        let mut num_added = 0;

        if offset == 0 {
            // increment before, offset in reply is offset to next entry
            num_added += 1;
            if reply.add(ino, num_added, FileType::Directory, ".") {
                return Ok(());
            }
        }

        if offset <= 1 {
            num_added += 1;
            let parent = inoman.parent(ino).ok_or(Errno::ENOENT)?;
            if reply.add(parent, num_added, FileType::Directory, "..") {
                return Ok(());
            }
        }

        for (i, entry_result) in fs::read_dir(&path)?
            .filter(|entry_result| {
                if let Ok(entry) = entry_result {
                    let path = entry.path();
                    if path.is_dir() {
                        self.include_dir(&path)
                            && !self.is_empty_dir(&path, self.prune_depth as isize)
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
            let entry = entry_result?;
            let path = entry.path();
            let ino = inoman.ino(&path);
            let kind = utils::filetype_of_path(&path)?;
            let name = path.file_name().ok_or(Errno::EIO)?;
            let name = name.to_str().ok_or(Errno::EIO)?;

            // return offset of next entry
            if reply.add(ino, i as u64 + offset + 1 + num_added, kind, name) {
                // buffer full
                return Ok(());
            }
        }

        Ok(())
    }
}

impl Filesystem for FilterFS {
    /// Get file attributes.
    fn getattr(&self, _req: &Request, ino: INodeNo, fh: Option<FileHandle>, reply: ReplyAttr) {
        debug!("Getting attributes for {}, fh: {:?}", ino, fh);
        match self.getattr(ino, fh.map(|fh| fh.0)) {
            Ok(attr) => reply.attr(&TTL, &attr),
            Err(err) => reply.error(err),
        };
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        debug!("lookup call on {:?} for parent {:?}", name, parent);

        match self.lookup(_req, parent, name) {
            Ok(attr) => reply.entry(&TTL, &attr, Generation(0)),
            Err(err) => reply.error(err),
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        debug!("open call on {:?}", ino);

        match self.open(_req, ino, flags) {
            Ok(fh) => reply.opened(fh, FopenFlags::empty()),
            Err(err) => reply.error(err),
        }
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
        debug!("release call on {:?}", fh);

        match self.fhman.lock() {
            Ok(mut fhman) => {
                fhman.release(fh.0);
                reply.ok();
            }
            Err(_) => {
                reply.error(Errno::EIO);
            }
        }
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
        match self.read(_req, fh.0, offset, size as usize, _flags) {
            Ok(buf) => reply.data(&buf),
            Err(err) => reply.error(err),
        }
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
        match self.readdir(_req, ino, fh.0, offset, &mut reply) {
            Ok(()) => {
                reply.ok();
            }
            Err(err) => reply.error(err),
        }
    }
}
