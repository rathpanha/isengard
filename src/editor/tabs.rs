/// An ordered list of open tabs with one active tab. Pure logic, independent of the UI.
pub struct TabList<T> {
    items: Vec<T>,
    active: usize,
}

impl<T> Default for TabList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            active: 0,
        }
    }
}

impl<T> TabList<T> {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn active_index(&self) -> Option<usize> {
        (!self.items.is_empty()).then_some(self.active)
    }

    pub fn active(&self) -> Option<&T> {
        self.items.get(self.active)
    }

    pub fn position(&self, pred: impl Fn(&T) -> bool) -> Option<usize> {
        self.items.iter().position(pred)
    }

    pub fn find_mut(&mut self, pred: impl Fn(&T) -> bool) -> Option<&mut T> {
        self.items.iter_mut().find(|t| pred(t))
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.items.get_mut(index)
    }

    /// Appends a tab and makes it active.
    pub fn push(&mut self, item: T) {
        self.items.push(item);
        self.active = self.items.len() - 1;
    }

    pub fn activate(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Removes a tab, keeping the same tab active where possible.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.items.len() {
            return None;
        }
        let item = self.items.remove(index);
        if index < self.active || self.active >= self.items.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some(item)
    }

    /// Moves the tab at `from` to index `to` (final index after the move).
    ///
    /// No-op when indexes are equal or out of range. Keeps the same tab active.
    pub fn reorder(&mut self, from: usize, to: usize) {
        let n = self.items.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        self.active = active_after_reorder(self.active, from, to);
    }

    pub fn cycle(&mut self, forward: bool) {
        let n = self.items.len();
        if n > 1 {
            self.active = if forward {
                (self.active + 1) % n
            } else {
                (self.active + n - 1) % n
            };
        }
    }
}

/// New `active` index after moving item `from` → `to` (final index).
pub fn active_after_reorder(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        to
    } else if from < to {
        // Moved right: (from, to] shift left by one.
        if active > from && active <= to {
            active - 1
        } else {
            active
        }
    } else {
        // Moved left: [to, from) shift right by one.
        if active >= to && active < from {
            active + 1
        } else {
            active
        }
    }
}

/// Map a drop "before `before_ix`" (left-border target) to `(from, to)` for
/// [`TabList::reorder`]. `None` when the drop would not change order.
pub fn reorder_for_drop_before(from: usize, before_ix: usize) -> Option<(usize, usize)> {
    if from == before_ix || from + 1 == before_ix {
        return None;
    }
    let to = if from < before_ix {
        before_ix - 1
    } else {
        before_ix
    };
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(names: &[&'static str]) -> TabList<&'static str> {
        let mut list = TabList::default();
        for name in names {
            list.push(*name);
        }
        list
    }

    fn names<'a>(list: &TabList<&'a str>) -> Vec<&'a str> {
        list.iter().copied().collect()
    }

    #[test]
    fn push_activates_new_tab() {
        let list = tabs(&["a", "b"]);
        assert_eq!(list.active(), Some(&"b"));
    }

    #[test]
    fn cycle_wraps_around() {
        let mut list = tabs(&["a", "b", "c"]);
        list.cycle(true);
        assert_eq!(list.active(), Some(&"a"));
        list.cycle(false);
        assert_eq!(list.active(), Some(&"c"));
    }

    #[test]
    fn remove_keeps_sensible_active_tab() {
        let mut list = tabs(&["a", "b", "c"]);
        list.remove(0);
        assert_eq!(list.active(), Some(&"c"));
        list.remove(1);
        assert_eq!(list.active(), Some(&"b"));
        list.remove(0);
        assert!(list.is_empty());
        assert_eq!(list.active(), None);
        assert_eq!(list.active_index(), None);
        assert_eq!(list.remove(0), None);
    }

    #[test]
    fn activate_ignores_out_of_range() {
        let mut list = tabs(&["a"]);
        assert!(!list.activate(3));
        assert_eq!(list.active(), Some(&"a"));
    }

    #[test]
    fn reorder_moves_right_and_keeps_active_item() {
        let mut list = tabs(&["a", "b", "c"]);
        list.activate(0);
        list.reorder(0, 2);
        assert_eq!(names(&list), ["b", "c", "a"]);
        assert_eq!(list.active(), Some(&"a"));
        assert_eq!(list.active_index(), Some(2));
    }

    #[test]
    fn reorder_moves_left_and_shifts_active() {
        let mut list = tabs(&["a", "b", "c"]);
        list.activate(0); // a
        list.reorder(2, 0); // c → front
        assert_eq!(names(&list), ["c", "a", "b"]);
        assert_eq!(list.active(), Some(&"a"));
        assert_eq!(list.active_index(), Some(1));
    }

    #[test]
    fn reorder_noop_on_same_or_oob() {
        let mut list = tabs(&["a", "b"]);
        list.activate(1);
        list.reorder(0, 0);
        list.reorder(5, 0);
        list.reorder(0, 5);
        assert_eq!(names(&list), ["a", "b"]);
        assert_eq!(list.active_index(), Some(1));
    }

    #[test]
    fn drop_before_maps_to_final_index() {
        assert_eq!(reorder_for_drop_before(0, 2), Some((0, 1)));
        assert_eq!(reorder_for_drop_before(2, 0), Some((2, 0)));
        assert_eq!(reorder_for_drop_before(0, 1), None);
        assert_eq!(reorder_for_drop_before(1, 1), None);
    }
}
