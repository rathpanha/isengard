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
}
