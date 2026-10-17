use super::{E, MenuHandle32, MenuStore32};

pub(crate) struct PreparedMenuRetirement32 {
    revision: u64,
    next_revision: u64,
    handles: Vec<MenuHandle32>,
}

impl MenuStore32 {
    pub(crate) fn prepare_retirement(
        &self,
        roots: impl IntoIterator<Item = MenuHandle32>,
    ) -> Result<PreparedMenuRetirement32, E> {
        let mut visited = [false; 256];
        for handle in roots {
            if let Ok(index) = self.index(handle) {
                self.collect_retirement(index, &mut visited);
            }
        }
        let count = visited[..self.menus.len()]
            .iter()
            .filter(|&&value| value)
            .count();
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(count)
            .map_err(|_| E::Allocation)?;
        for (index, menu) in self.menus.iter().enumerate() {
            if visited[index] {
                handles.push(menu.handle);
            }
        }
        Ok(PreparedMenuRetirement32 {
            revision: self.revision,
            next_revision: if handles.is_empty() {
                self.revision
            } else {
                self.next_revision()?
            },
            handles,
        })
    }

    fn collect_retirement(&self, index: usize, visited: &mut [bool; 256]) {
        if visited[index] {
            return;
        }
        visited[index] = true;
        for child in self.menus[index]
            .items
            .iter()
            .filter_map(|item| item.submenu)
        {
            if let Ok(index) = self.index(child) {
                self.collect_retirement(index, visited);
            }
        }
    }

    pub(crate) fn check_retirement(&self, receipt: &PreparedMenuRetirement32) -> Result<(), E> {
        if self.revision != receipt.revision {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }

    pub(crate) fn commit_retirement(&mut self, receipt: PreparedMenuRetirement32) {
        self.check_retirement(&receipt)
            .expect("prepared menu retirement is current");
        self.destroy_collected(&receipt.handles);
        self.revision = receipt.next_revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::user32::menus::MenuItem32;
    fn child(id: u32, submenu: MenuHandle32) -> MenuItem32 {
        MenuItem32 {
            submenu: Some(submenu),
            ..MenuItem32::command(id, Vec::new())
        }
    }

    #[test]
    fn prepared_union_destroys_attached_roots_once_and_keeps_replaced_and_unrelated_menus() {
        let mut menus = MenuStore32::new(Default::default()).unwrap();
        let replaced = menus.create(false).unwrap();
        let first = menus.create(false).unwrap();
        let second = menus.create(false).unwrap();
        let shared = menus.create(true).unwrap();
        menus.insert(first, 0, child(1, shared)).unwrap();
        menus.insert(second, 0, child(2, shared)).unwrap();
        let version = menus.revision();
        let receipt = menus.prepare_retirement([first, second, first]).unwrap();
        assert!(menus.contains(first));
        assert!(menus.contains(shared));
        menus.commit_retirement(receipt);
        assert_eq!(menus.revision(), version + 1);
        assert!(menus.contains(replaced));
        for handle in [first, second, shared] {
            assert!(!menus.contains(handle));
        }
    }

    #[test]
    fn stale_preparation_and_revision_exhaustion_leave_menu_owner_and_children_intact() {
        let mut menus = MenuStore32::new(Default::default()).unwrap();
        let root = menus.create(false).unwrap();
        let child_menu = menus.create(true).unwrap();
        menus.insert(root, 0, child(1, child_menu)).unwrap();
        let receipt = menus.prepare_retirement([root]).unwrap();
        menus.set_help(root, 17).unwrap();
        assert_eq!(menus.check_retirement(&receipt), Err(E::InvalidRequest));
        assert!(menus.contains(root));
        assert!(menus.contains(child_menu));
        menus.revision = u64::MAX;
        assert!(matches!(menus.prepare_retirement([root]), Err(E::Capacity)));
        assert!(menus.contains(root));
        assert!(menus.contains(child_menu));
        let neutral = menus.prepare_retirement([MenuHandle32(1)]).unwrap();
        menus.commit_retirement(neutral);
        assert_eq!(menus.revision(), u64::MAX);
    }
}
