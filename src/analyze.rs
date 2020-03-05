use crate::MarkedData;
use std::cmp::Eq;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;

pub fn analyze<'a>(olds: &'a [AnnotatedMark], news: &'a [AnnotatedMark]) -> Analysis<'a> {
  let olds_order: Vec<u32> = olds.iter().map(|m| m.id).collect();
  let news_order: Vec<u32> = news.iter().map(|m| m.id).collect();

  let olds_map: HashMap<u32, &'a AnnotatedMark> = olds.iter().map(|m| (m.id, m)).collect();
  let news_map: HashMap<u32, &'a AnnotatedMark> = news.iter().map(|m| (m.id, m)).collect();

  let olds_ids: HashSet<u32> = olds.iter().map(|m| m.id).collect();
  let news_ids: HashSet<u32> = news.iter().map(|m| m.id).collect();
  let olds_only = difference(&olds_order, &news_ids);
  let news_only = difference(&news_order, &olds_ids);
  let both_ids = intersection(&news_order, &olds_ids);

  let changes = both_ids.iter().map(|i| Change::calc(olds_map[i], news_map[i])).collect();

  Analysis {
    newer: news_only.iter().map(|i| news_map[i]).collect(),
    older: olds_only.iter().map(|i| olds_map[i]).collect(),
    changes
  }
}

pub struct AnnotatedMark {
  id: u32,
  name: String,
  mark: MarkedData
}

impl AnnotatedMark {
  pub fn new(id: u32, name: String, mark: MarkedData) -> AnnotatedMark { AnnotatedMark { id, name, mark } }
  pub fn id(&self) -> u32 { self.id }
  pub fn name(&self) -> &str { &self.name }
  pub fn mark(&self) -> &MarkedData { &self.mark }
}

pub struct Analysis<'a> {
  newer: Vec<&'a AnnotatedMark>,
  older: Vec<&'a AnnotatedMark>,
  changes: Vec<Change<'a>>
}

impl<'a> Analysis<'a> {
  pub fn newer(&self) -> &Vec<&'a AnnotatedMark> { &self.newer }
  pub fn older(&self) -> &Vec<&'a AnnotatedMark> { &self.older }
  pub fn changes(&self) -> &Vec<Change<'a>> { &self.changes }
}

pub struct Change<'a> {
  old_mark: &'a AnnotatedMark,
  new_mark: &'a AnnotatedMark,
  name: Option<(&'a str, &'a str)>,
  value: Option<(&'a str, &'a str)>
}

impl<'a> Change<'a> {
  pub fn new(
    old_mark: &'a AnnotatedMark, new_mark: &'a AnnotatedMark, name: Option<(&'a str, &'a str)>,
    value: Option<(&'a str, &'a str)>
  ) -> Change<'a> {
    Change { old_mark, new_mark, name, value }
  }

  pub fn calc(old: &'a AnnotatedMark, new: &'a AnnotatedMark) -> Change<'a> {
    let name = if old.name() == new.name() { None } else { Some((old.name(), new.name())) };

    let value =
      if old.mark.value() == new.mark().value() { None } else { Some((old.mark().value(), new.mark().value())) };

    Change::new(old, new, name, value)
  }

  pub fn old_mark(&self) -> &'a AnnotatedMark { self.old_mark }
  pub fn new_mark(&self) -> &'a AnnotatedMark { self.new_mark }
  pub fn name(&self) -> &Option<(&'a str, &'a str)> { &self.name }
  pub fn value(&self) -> &Option<(&'a str, &'a str)> { &self.value }
}

fn difference<T: Copy + Eq + Hash>(o1: &[T], o2: &HashSet<T>) -> Vec<T> {
  o1.iter().filter(|t| !o2.contains(t)).copied().collect()
}

fn intersection<T: Copy + Eq + Hash>(o1: &[T], o2: &HashSet<T>) -> Vec<T> {
  o1.iter().filter(|t| o2.contains(t)).copied().collect()
}
