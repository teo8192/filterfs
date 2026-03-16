use std::ops::AddAssign;

pub struct IdManager<T>
where
    T: Default + AddAssign<u64> + Clone,
{
    released: Vec<T>,
    next_id: T,
}

impl<T> IdManager<T>
where
    T: Default + AddAssign<u64> + Clone,
{
    pub fn new() -> Self {
        Self {
            released: Vec::new(),
            next_id: Default::default(),
        }
    }

    pub fn get(&mut self) -> T {
        if let Some(id) = self.released.pop() {
            return id;
        }

        let next_id = self.next_id.clone();
        self.next_id += 1;
        next_id
    }

    pub fn release(&mut self, id: T) {
        self.released.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::IdManager;

    #[test]
    fn allocations() {
        let mut idman: IdManager<u64> = IdManager::new();

        let id = idman.get();

        assert_eq!(id, 0);
    }

    #[test]
    fn releases() {
        let mut idman: IdManager<u64> = IdManager::new();

        assert_eq!(idman.released.len(), 0);

        let id = idman.get();
        idman.release(id);

        assert_eq!(idman.released.len(), 1);

        let id = idman.get();

        assert_eq!(id, 0);
        assert_eq!(idman.released.len(), 0);

        let id2 = idman.get();

        assert_eq!(id2, 1);
        assert_eq!(idman.released.len(), 0);

        idman.release(id);

        assert_eq!(idman.released.len(), 1);

        let id = idman.get();

        assert_eq!(id, 0);
    }
}
