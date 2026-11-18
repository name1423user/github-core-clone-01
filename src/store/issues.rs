//! Labels, issues and comments. Call with the repo locked (`store::lock`). 持ち場: リード。

use super::{store, Comment, Issue, Kind, Label, Repo, StoreError};

impl Repo {
    pub fn next_number(&mut self) -> u64 {
        self.last_number += 1;
        self.last_number
    }

    pub fn label_index(&self, name: &str) -> Option<usize> {
        // GitHub matches label names case-insensitively.
        self.labels.iter().position(|l| l.name.eq_ignore_ascii_case(name))
    }

    pub fn create_label(&mut self, name: &str, color: &str, description: &str) -> Result<&Label, StoreError> {
        if self.label_index(name).is_some() {
            return Err(StoreError::AlreadyExists);
        }
        let id = store().next_id(Kind::Label);
        self.labels.push(Label { id, name: name.to_string(), color: color.to_string(), description: description.to_string() });
        Ok(self.labels.last().unwrap())
    }

    /// Every label must exist already (unknown → `UnknownLabel`, REST 422).
    pub fn create_issue(&mut self, title: &str, body: &str, labels: &[&str]) -> Result<&mut Issue, StoreError> {
        let labels = labels
            .iter()
            .map(|n| self.label_index(n).ok_or_else(|| StoreError::UnknownLabel(n.to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        let number = self.next_number();
        let id = store().next_id(Kind::Issue);
        self.issues.push(Issue {
            id,
            number,
            title: title.to_string(),
            body: body.to_string(),
            open: true,
            labels,
            comments: Vec::new(),
            json: None,
        });
        Ok(self.issues.last_mut().unwrap())
    }

    /// Issues are pushed in number order, so a binary search finds one.
    pub fn issue(&self, number: u64) -> Option<&Issue> {
        self.issues.binary_search_by_key(&number, |i| i.number).ok().map(|i| &self.issues[i])
    }

    /// Mutable access drops the cached JSON: every change goes through here.
    pub fn issue_mut(&mut self, number: u64) -> Option<&mut Issue> {
        let i = self.issues.binary_search_by_key(&number, |i| i.number).ok()?;
        let issue = &mut self.issues[i];
        issue.json = None;
        Some(issue)
    }

    pub fn issue_by_id(&self, id: u64) -> Option<&Issue> {
        self.issues.iter().find(|i| i.id == id)
    }

    pub fn add_comment(&mut self, number: u64, body: &str) -> Result<&Comment, StoreError> {
        let issue = self.issue_mut(number).ok_or(StoreError::NotFound)?;
        issue.comments.push(Comment { id: store().next_id(Kind::Comment), body: body.to_string() });
        Ok(issue.comments.last().unwrap())
    }

    /// Open/closed filter (`None` = all), ascending by number (= creation order; all share the trial's now).
    pub fn list_issues(&self, open: Option<bool>) -> impl DoubleEndedIterator<Item = &Issue> {
        self.issues.iter().filter(move |i| open.is_none_or(|o| i.open == o))
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn issues_and_labels() {
        let s = store();
        let r = s.create_repo("t-issues", "r", "", false).unwrap();
        assert_eq!(s.create_repo("t-issues", "r", "", false).err(), Some(StoreError::AlreadyExists));
        let mut r = lock(&r);
        r.create_label("bug", "f00", "").unwrap();
        assert!(r.create_label("BUG", "f00", "").is_err());
        assert_eq!(r.create_issue("x", "", &["nope"]).err().map(|_| ()), Some(()));
        let a = r.create_issue("a", "b", &["bug"]).unwrap().number;
        let b = r.create_issue("c", "d", &[]).unwrap().number;
        assert_eq!(b, a + 1);
        r.issue_mut(a).unwrap().open = false;
        assert_eq!(r.list_issues(Some(true)).map(|i| i.number).collect::<Vec<_>>(), vec![b]);
        assert_eq!(r.list_issues(None).rev().map(|i| i.number).collect::<Vec<_>>(), vec![b, a]);
        let c = r.add_comment(b, "hi").unwrap().id;
        assert_eq!(r.issue(b).unwrap().comments[0].id, c);
        assert!(r.add_comment(999, "x").is_err());
    }
}
