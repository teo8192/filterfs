use std::{collections::HashMap, path::{Path, PathBuf}};

use fuser::INodeNo;

#[derive(Clone)]
pub struct INode {
    // id: INodeNo,
    parent: INodeNo,
    pub path: PathBuf,
}

pub struct INodeManager {
    path_to_inode: HashMap<PathBuf, INodeNo>,
    inodes: HashMap<INodeNo, INode>,
    next_inode: INodeNo,
}

impl INodeManager {
    pub fn new(root: &Path) -> Self {
        let mut path_to_inode = HashMap::new();
        let mut inodes = HashMap::new();
        path_to_inode.insert(root.to_path_buf(), INodeNo(1));
        let inode = INode {
            // id: INodeNo(1),
            path: root.to_path_buf(),
            parent: INodeNo(1),
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

    pub fn ino(&mut self, path: &Path) -> INodeNo {
        if let Some(ino) = self.path_to_inode.get(path) {
            return *ino;
        }

        // need to get parent ino
        let pino = match path.parent() {
            Some(p) => self.ino(p),
            None => {
                return INodeNo(1); // has no parent, assume root
            }
        };

        // need to allocate a new inode (and perhaps look for parent?)
        let nino = self.next_inode();
        let inode = INode {
            // id: nino,
            path: path.to_path_buf(),
            parent: pino,
        };

        self.path_to_inode.insert(path.to_path_buf(), nino);
        self.inodes.insert(nino, inode);

        nino
    }

    pub fn inode(&self, ino: INodeNo) -> Option<INode> {
        self.inodes.get(&ino).cloned()
    }

    pub fn path(&self, ino: INodeNo) -> Option<PathBuf> {
        self.inodes.get(&ino).map(|v| v.path.clone())
    }

    pub fn parent(&self, ino: INodeNo) -> Option<INodeNo> {
        self.inodes.get(&ino).map(|v| v.parent)
    }
}
