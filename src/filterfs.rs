use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::FileExt;
use std::path::Path;
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
        debug!("lookup call on {:?} for parent {:?}", name, parent);

        let mut inoman = autorep!(self.inoman.lock().ok(), reply);
        let path = autorep!(inoman.path(parent), reply).join(name);

        if !self.include_file(&path) {
            reply.error(Errno::ENOENT);
            return;
        }

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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Once;
    use std::{fs, io};

    use fuser::{Config, MountOption};
    use log::Level;
    use tempfile::{tempdir, TempDir};

    use crate::pattern::PatternRule;

    use super::FilterFS;

    const FS_NAME: &str = "filterfs";

    static INIT: Once = Once::new();

    fn global_setup() {
        INIT.call_once(|| {
            simple_logger::init_with_level(Level::Trace).unwrap();
        });
    }

    struct FsTester {
        source: TempDir,
        mountpoint: TempDir,
    }

    impl FsTester {
        fn new() -> io::Result<Self> {
            Ok(Self {
                source: tempdir()?,
                mountpoint: tempdir()?,
            })
        }

        fn source(&self) -> &Path {
            self.source.path()
        }

        fn mountpoint(&self) -> &Path {
            self.mountpoint.path()
        }

        fn add_dir(&mut self, path: &str) {
            let path = PathBuf::from(path);
            // let self be mut to signify that underlying resource is modified
            let _ = fs::create_dir_all(self.source.path().join(path));
        }

        fn add_file(&mut self, path: &str, contents: &str) {
            let path = PathBuf::from(path);
            // let self be mut to signify that underlying resource is modified
            let parent = path.parent().unwrap();
            let _ = fs::create_dir_all(self.source.path().join(parent));
            let _ = fs::write(self.source.path().join(path), contents);
        }

        fn read_file(&self, path: &str) -> Result<String, io::Error> {
            let path = PathBuf::from(path);
            fs::read(self.mountpoint.path().join(path))
                .map(|content| String::from_utf8(content).unwrap())
        }

        fn read_dir(&self, path: &str) -> Option<HashSet<String>> {
            let path = PathBuf::from(path);
            let mut entries = HashSet::new();
            for entry in fs::read_dir(self.mountpoint.path().join(path)).ok()? {
                let entry = entry.ok()?;
                entries.insert(entry.path().file_name()?.to_str()?.to_string());
            }

            Some(entries)
        }
    }

    macro_rules! test_options {
        () => {{
            let mut options = Config::default();
            options.mount_options = vec![MountOption::FSName(FS_NAME.to_string())];
            options
        }};
    }

    #[test]
    fn test_empty() {
        global_setup();

        // setup
        let fst = FsTester::new().unwrap();
        let expected: HashSet<String> = HashSet::new();

        // start filesystem
        let fs = FilterFS::new(fst.source(), 0, vec![], vec![], vec![], vec![]);
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // asserts
        assert_eq!(expected, fst.read_dir("").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_transparent() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();
        let mut expected: HashSet<String> = HashSet::new();

        fst.add_file("doc1.pdf", "");
        expected.insert("doc1.pdf".to_string());

        fst.add_file("doc2.pdf", "");
        expected.insert("doc2.pdf".to_string());

        fst.add_file("file.txt", "");
        expected.insert("file.txt".to_string());

        fst.add_file("whatever", "");
        expected.insert("whatever".to_string());

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");
        expected.insert("lol".to_string());

        fst.add_dir("1");
        expected.insert("1".to_string());

        // start filesystem
        let fs = FilterFS::new(fst.source(), 0, vec![], vec![], vec![], vec![]);
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert_eq!(expected, fst.read_dir("").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_onlypdf() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();
        let mut expected: HashSet<String> = HashSet::new();

        fst.add_file("doc1.pdf", "");
        expected.insert("doc1.pdf".to_string());

        fst.add_file("doc2.pdf", "");
        expected.insert("doc2.pdf".to_string());

        fst.add_file("file.txt", "");

        fst.add_file("whatever", "");

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");
        expected.insert("lol".to_string());

        fst.add_dir("1");
        expected.insert("1".to_string());

        // start filesystem
        let fs = FilterFS::new(
            fst.source(),
            0,
            vec![PatternRule::new_include("*.pdf").unwrap()],
            vec![],
            vec![],
            vec![],
        );
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert_eq!(expected, fst.read_dir("").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_nopdf() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();
        let mut expected: HashSet<String> = HashSet::new();

        fst.add_file("doc1.pdf", "");

        fst.add_file("doc2.pdf", "");

        fst.add_file("file.txt", "");
        expected.insert("file.txt".to_string());

        fst.add_file("whatever", "");
        expected.insert("whatever".to_string());

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");
        expected.insert("lol".to_string());

        fst.add_dir("1");
        expected.insert("1".to_string());

        // start filesystem
        let fs = FilterFS::new(
            fst.source(),
            0,
            vec![],
            vec![PatternRule::new_exclude("*.pdf").unwrap()],
            vec![],
            vec![],
        );
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert_eq!(expected, fst.read_dir("").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_low_prune() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();
        let mut expected: HashSet<String> = HashSet::new();

        fst.add_file("doc1.pdf", "");

        fst.add_file("doc2.pdf", "");

        fst.add_file("file.txt", "");
        expected.insert("file.txt".to_string());

        fst.add_file("whatever", "");
        expected.insert("whatever".to_string());

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");
        expected.insert("lol".to_string());

        fst.add_dir("1");

        // start filesystem
        let fs = FilterFS::new(
            fst.source(),
            1,
            vec![],
            vec![PatternRule::new_exclude("*.pdf").unwrap()],
            vec![],
            vec![],
        );
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert_eq!(expected, fst.read_dir("").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_high_prune() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();
        let mut expected: HashSet<String> = HashSet::new();

        fst.add_file("doc1.pdf", "");

        fst.add_file("doc2.pdf", "");

        fst.add_file("file.txt", "");
        expected.insert("file.txt".to_string());

        fst.add_file("whatever", "");
        expected.insert("whatever".to_string());

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");

        fst.add_dir("1");

        // start filesystem
        let fs = FilterFS::new(
            fst.source(),
            5,
            vec![],
            vec![PatternRule::new_exclude("*.pdf").unwrap()],
            vec![],
            vec![],
        );
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert_eq!(expected, fst.read_dir("").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_file_content_of_acceptable_file() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();

        fst.add_file("doc1.pdf", "what");
        fst.add_file("doc2.pdf", "hello");
        fst.add_file("file.txt", "shit content");
        fst.add_file("whatever", "lol");

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");

        fst.add_dir("1");

        // start filesystem
        let fs = FilterFS::new(
            fst.source(),
            5,
            vec![],
            vec![PatternRule::new_exclude("*.pdf").unwrap()],
            vec![],
            vec![],
        );
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert_eq!("shit content", fst.read_file("file.txt").unwrap());

        handle.umount_and_join().unwrap();
    }

    #[test]
    fn test_file_content_of_filtered_file() {
        global_setup();

        // setup
        let mut fst = FsTester::new().unwrap();

        fst.add_file("doc1.pdf", "what");
        fst.add_file("doc2.pdf", "hello");
        fst.add_file("file.txt", "shit content");
        fst.add_file("whatever", "lol");

        fst.add_dir("lol/wtf/is/this");
        fst.add_dir("lol/wtf/thing/this");
        fst.add_dir("lol/xd/thing/this");
        fst.add_dir("lol/1/thing/this");

        fst.add_dir("1");

        // start filesystem
        let fs = FilterFS::new(
            fst.source(),
            5,
            vec![],
            vec![PatternRule::new_exclude("*.pdf").unwrap()],
            vec![],
            vec![],
        );
        let options = test_options!();
        let handle = fuser::spawn_mount2(fs, fst.mountpoint(), &options).unwrap();

        // sleep for a bit to let the filesystem start up

        // asserts
        assert!(fst.read_file("doc1.pdf").is_err());

        handle.umount_and_join().unwrap();
    }
}
