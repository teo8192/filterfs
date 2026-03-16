use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use clap::Parser;
use fuser::{
    Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo,
    LockOwner, MountOption, OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, Request,
};
use log::debug;

use crate::id::IdManager;

mod id;

/// FilterFS
#[derive(Parser)]
struct Args {
    /// Underlying source directory
    source: PathBuf,

    /// Mount point
    mountpoint: PathBuf,

    /// Run in foreground
    #[arg(short, long)]
    foreground: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Extensions to include, ',' separated list
    /// if nothing is given, everything is included
    #[arg(short, long)]
    include: Option<String>,

    /// FUSE-style mount options
    /// i.e. -o include=so,include=TAG
    /// will yield includes for both .so and .TAG and nothing else
    #[arg(short = 'o')]
    options: Option<String>,
}

const TTL: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct INode {
    // id: INodeNo,
    // parent: INodeNo,
    path: PathBuf,
}

struct INodeManager {
    path_to_inode: HashMap<PathBuf, INodeNo>,
    inodes: HashMap<INodeNo, INode>,
    next_inode: INodeNo,
}

impl INodeManager {
    fn new(root: &Path) -> Self {
        let mut path_to_inode = HashMap::new();
        let mut inodes = HashMap::new();
        path_to_inode.insert(root.to_path_buf(), INodeNo(1));
        let inode = INode {
            // id: INodeNo(1),
            path: root.to_path_buf(),
            // parent: INodeNo(1),
        };

        inodes.insert(INodeNo(1), inode);

        Self {
            path_to_inode,
            inodes,
            next_inode: INodeNo(2),
        }
    }

    fn next_inode(&mut self) -> INodeNo {
        let INodeNo(ino) = self.next_inode;
        self.next_inode = INodeNo(ino + 1);
        INodeNo(ino)
    }

    fn ino(&mut self, path: &Path) -> INodeNo {
        if let Some(ino) = self.path_to_inode.get(path) {
            return *ino;
        }

        // need to get parent ino
        /*let pino = match path.parent() {
            Some(p) => self.ino(p),
            None => {
                return INodeNo(1); // has no parent, assume root
            }
        };*/

        // need to allocate a new inode (and perhaps look for parent?)
        let nino = self.next_inode();
        let inode = INode {
            // id: nino,
            path: path.to_path_buf(),
            // parent: pino,
        };

        self.path_to_inode.insert(path.to_path_buf(), nino);
        self.inodes.insert(nino, inode);

        nino
    }

    fn inode(&self, ino: INodeNo) -> Option<INode> {
        self.inodes.get(&ino).cloned()
    }

    fn path(&self, ino: INodeNo) -> Option<PathBuf> {
        self.inodes.get(&ino).map(|v| v.path.clone())
    }

    /*
    fn parent(&self, ino: INodeNo) -> Option<INodeNo> {
        self.inodes.get(&ino).map(|v| v.parent)
    }*/
}

type FhId = u64;

struct Handle {
    // inode: INode,
    handle: fs::File,
}

struct FhManager {
    idman: IdManager<FhId>,
    handle_to_inode: HashMap<FhId, Handle>,
}

impl FhManager {
    pub fn new() -> Self {
        Self {
            idman: IdManager::new(),
            handle_to_inode: HashMap::new(),
        }
    }

    pub fn open(&mut self, inode: INode) -> Option<FhId> {
        let fhid = self.idman.get();
        let fh = fs::File::open(&inode.path).ok()?;
        let handle = Handle {
            //inode,
            handle: fh,
        };

        self.handle_to_inode.insert(fhid, handle);

        Some(fhid)
    }

    pub fn release(&mut self, id: FhId) {
        self.handle_to_inode.remove(&id);
        self.idman.release(id);
    }

    pub fn handle(&self, id: FhId) -> Option<&Handle> {
        self.handle_to_inode.get(&id)
    }
}

struct FilterFS {
    // root: PathBuf,
    include: HashSet<String>,
    inoman: Mutex<INodeManager>,
    fhman: Mutex<FhManager>,
}

impl FilterFS {
    fn new(root: PathBuf, include: HashSet<String>) -> Self {
        let inoman = INodeManager::new(&root);
        Self {
            // root,
            include,
            inoman: Mutex::new(inoman),
            fhman: Mutex::new(FhManager::new()),
        }
    }

    fn include_file(&self, file: &PathBuf) -> bool {
        if self.include.is_empty() {
            return true;
        }

        if let Some(ext) = file.extension() && let Some(ext) = ext.to_str() {
            self.include.contains(ext)
        } else {
            false
        }
    }
}

macro_rules! time {
    ($time:expr) => {
        UNIX_EPOCH + Duration::from_secs($time as u64)
    };
}

fn filetype(metadata: &fs::Metadata) -> FileType {
    match metadata.mode() & libc::S_IFMT {
        libc::S_IFREG => FileType::RegularFile,
        libc::S_IFDIR => FileType::Directory,
        libc::S_IFLNK => FileType::Symlink,
        libc::S_IFCHR => FileType::CharDevice,
        libc::S_IFBLK => FileType::BlockDevice,
        libc::S_IFIFO => FileType::NamedPipe,
        libc::S_IFSOCK => FileType::Socket,
        _ => FileType::RegularFile, // default to regular file
    }
}

fn filetype_of_path(path: &PathBuf) -> Option<FileType> {
    Some(filetype(&fs::metadata(&path).ok()?))
}

fn permissions(perm: fs::Permissions) -> u16 {
    (perm.mode() & 0o7777) as u16 // keep the lower 12 bits
}

macro_rules! cutoff {
    ($num:expr) => {
        ::std::cmp::min($num, u32::MAX as u64) as u32
    };
}

fn convert_metadata(metadata: &fs::Metadata, ino: Option<INodeNo>) -> FileAttr {
    let ino = match ino {
        Some(ino) => ino,
        None => INodeNo(metadata.ino()),
    };
    FileAttr {
        ino,
        size: metadata.size(),
        blocks: metadata.blocks(),
        atime: time!(metadata.atime()),
        mtime: time!(metadata.mtime()),
        ctime: time!(metadata.ctime()),
        crtime: UNIX_EPOCH, // macos only
        kind: filetype(metadata),
        perm: permissions(metadata.permissions()),
        nlink: cutoff!(metadata.nlink()),
        uid: metadata.uid(),
        gid: metadata.gid(),
        rdev: cutoff!(metadata.rdev()),
        blksize: cutoff!(metadata.blksize()),
        flags: 0, //macos only
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

        let attr: FileAttr = convert_metadata(&metadata, Some(ino));
        reply.attr(&TTL, &attr);
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let mut inoman = autorep!(self.inoman.lock().ok(), reply);
        let mut path = autorep!(inoman.path(parent), reply);
        path.push(name);
        let ino = inoman.ino(&path);
        let metadata = autorep!(fs::metadata(&path).ok(), reply, Errno::ENOENT);
        reply.entry(&TTL, &convert_metadata(&metadata, Some(ino)), Generation(0));
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
                    let path  = entry.path();
                    path.is_dir() || self.include_file(&path)
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
            let kind = autorep!(filetype_of_path(&path), reply);
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

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let mut is_debug = env::var("RUST_LOG").is_err() && args.debug;

    let mut include = HashSet::new();
    if let Some(include_str) = args.include {
        for ext in include_str.split(",") {
            include.insert(ext.to_string());
        }
    }

    if let Some(options) = args.options {
        for option in options.split(',') {
            let mut option = option.split('=');
            match option.next() {
                Some("include") => {
                    include.insert(option.next().unwrap().to_string());
                }
                Some("debug") => {
                    is_debug = true;
                }
                _ => {
                    panic!("unknown option");
                }
            }
        }
    }

    if is_debug {
        unsafe { env::set_var("RUST_LOG", "debug") };
    }
    env_logger::init();


    let filesys = FilterFS::new(args.source, include);
    let mut options = Config::default();
    options.mount_options = vec![MountOption::FSName("filterfs".to_string())];

    Ok(fuser::mount2(filesys, args.mountpoint, &options)?)
}
