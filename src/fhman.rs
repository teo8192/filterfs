use std::collections::HashMap;
use std::fs;

use crate::id::IdManager;
use crate::inoman::INode;

pub type FhId = u64;

pub struct Handle {
    // inode: INode,
    pub handle: fs::File,
}

pub struct FhManager {
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

impl Default for FhManager {
    fn default() -> Self {
        Self::new()
    }
}
