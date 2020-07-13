use crate::source::MarkedData;
use std::collections::HashMap;
use std::collections::HashSet;

pub fn analyze(olds: Vec<AnnotatedMark>, news: Vec<AnnotatedMark>) -> Analysis {
  let olds_ids: HashSet<u32> = olds.iter().map(|m| m.id).collect();
  let news_ids: HashSet<u32> = news.iter().map(|m| m.id).collect();

  let (older, old_matches): (Vec<_>, Vec<_>) = olds.into_iter().partition(|m| !news_ids.contains(&m.id));
  let (newer, new_matches): (Vec<_>, Vec<_>) = news.into_iter().partition(|m| !olds_ids.contains(&m.id));

  let old_matches: HashMap<_, _> = old_matches.into_iter().map(|m| (m.id, m)).collect();
  let mut new_matches: HashMap<_, _> = new_matches.into_iter().map(|m| (m.id, m)).collect();

  let changes = old_matches.into_iter().map(|(id, o)| Change::calc(o, new_matches.remove(&id).unwrap())).collect();

  Analysis { newer, older, changes }
}

pub struct AnnotatedMark {
  id: u32,
  name: String,
  mark: MarkedData
}

impl AnnotatedMark {
  pub fn new(id: u32, name: String, mark: MarkedData) -> AnnotatedMark { AnnotatedMark { id, name, mark } }
  pub fn name(&self) -> &str { &self.name }
  pub fn mark(&self) -> &MarkedData { &self.mark }
}

pub struct Analysis {
  newer: Vec<AnnotatedMark>,
  older: Vec<AnnotatedMark>,
  changes: Vec<Change>
}

impl Analysis {
  pub fn newer(&self) -> &Vec<AnnotatedMark> { &self.newer }
  pub fn older(&self) -> &Vec<AnnotatedMark> { &self.older }
  pub fn changes(&self) -> &Vec<Change> { &self.changes }
}

pub struct Change {
  old_mark: AnnotatedMark,
  new_mark: AnnotatedMark,
  name_change: bool,
  value_change: bool
}

impl Change {
  pub fn calc(old_mark: AnnotatedMark, new_mark: AnnotatedMark) -> Change {
    let name_change = old_mark.name() != new_mark.name();
    let value_change = old_mark.mark.value() != new_mark.mark().value();

    Change { old_mark, new_mark, name_change, value_change }
  }

  pub fn new_mark(&self) -> &AnnotatedMark { &self.new_mark }
  pub fn old_mark(&self) -> &AnnotatedMark { &self.old_mark }

  pub fn name(&self) -> Option<(&str, &str)> {
    if self.name_change {
      Some((self.old_mark.name(), self.new_mark.name()))
    } else {
      None
    }
  }

  pub fn value(&self) -> Option<(&str, &str)> {
    if self.value_change {
      Some((self.old_mark.mark.value(), self.new_mark.mark.value()))
    } else {
      None
    }
  }
}

// fn difference<T: Copy + Eq + Hash>(o1: &[T], o2: &HashSet<T>) -> Vec<T> {
//   o1.iter().filter(|t| !o2.contains(t)).copied().collect()
// }
//
// fn intersection<T: Copy + Eq + Hash>(o1: &[T], o2: &HashSet<T>) -> Vec<T> {
//   o1.iter().filter(|t| o2.contains(t)).copied().collect()
// }
