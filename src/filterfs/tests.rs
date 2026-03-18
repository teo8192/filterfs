use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
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

    fn set_permissions(&self, path: &str, perm: fs::Permissions) {
        let path = PathBuf::from(path);
        fs::set_permissions(self.source.path().join(path), perm).unwrap();
    }

    fn check_permissions(&self, path: &str) {
        let path = PathBuf::from(path);

        let orig_file = self.source.path().join(&path);
        let dest_file = self.mountpoint.path().join(&path);

        let orig_metadata = fs::metadata(orig_file).unwrap();
        let dest_metadata = fs::metadata(dest_file).unwrap();

        assert_eq!(orig_metadata.size(), dest_metadata.size());
        assert_eq!(orig_metadata.blocks(), dest_metadata.blocks());
        assert_eq!(orig_metadata.mtime(), dest_metadata.mtime());
        assert_eq!(orig_metadata.ctime(), dest_metadata.ctime());
        assert_eq!(orig_metadata.file_type(), dest_metadata.file_type());
        assert_eq!(orig_metadata.permissions(), dest_metadata.permissions());
        assert_eq!(orig_metadata.nlink(), dest_metadata.nlink());
        assert_eq!(orig_metadata.uid(), dest_metadata.uid());
        assert_eq!(orig_metadata.gid(), dest_metadata.gid());
        assert_eq!(orig_metadata.rdev(), dest_metadata.rdev());
        assert_eq!(orig_metadata.blksize(), dest_metadata.blksize());
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
fn test_emptydir() {
    global_setup();

    // setup
    let mut fst = FsTester::new().unwrap();
    let expected: HashSet<String> = HashSet::new();

    fst.add_file("doc1.pdf", "");

    fst.add_file("doc2.pdf", "");

    fst.add_file("file.txt", "");

    fst.add_file("whatever", "");

    fst.add_dir("lol/wtf/is/this");
    fst.add_dir("lol/wtf/thing/this");
    fst.add_dir("lol/xd/thing/this");
    fst.add_dir("lol/1/thing/this");

    fst.add_dir("1");

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
    assert_eq!(expected, fst.read_dir("1").unwrap());

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

#[test]
fn same_permissions_simple() {
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
    fst.check_permissions("file.txt");
    fst.check_permissions("lol");

    handle.umount_and_join().unwrap();
}
