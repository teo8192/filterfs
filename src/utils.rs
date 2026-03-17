use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use fuser::{FileAttr, FileType, INodeNo};

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

pub fn filetype_of_path(path: &PathBuf) -> Option<FileType> {
    Some(filetype(&fs::metadata(path).ok()?))
}

fn permissions(perm: fs::Permissions) -> u16 {
    (perm.mode() & 0o7777) as u16 // keep the lower 12 bits
}

macro_rules! cutoff {
    ($num:expr) => {
        ::std::cmp::min($num, u32::MAX as u64) as u32
    };
}

pub fn convert_metadata(metadata: &fs::Metadata, ino: INodeNo) -> FileAttr {
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
