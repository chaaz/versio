//! Interactions with github API v4.

use crate::errors::Result;
use crate::git::{time_to_datetime, Auth, CommitInfoBuf, FromTag, FromTagBuf, FullPr, GithubInfo, Repo, Span};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use octocrab::Octocrab;
use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

/// Find all changes in a repo more cleverly than `git rev-parse begin..end` using the GitHub v4 GraphQL API.
///
/// This method groups the commits into pull requests (PRs), starting with "PR zero" (which is an artificial
/// group that contains all commits in the given range) and for each commit, ask the GitHub API for "associated
/// pull requests". For each such associated PR, it performs a rev-parse on the base/head of that PR to search
/// for more commits, and continues recursively. Each commit found may placed into more than one PR.
///
/// When a commit is found where it itself does not belong to one of its own associated PRs' "base..head"
/// rev-parse, we assume that this is the result of a "squash merge" from that PR (or some other type of PR
/// rebase). The squash commit is excluded from all PRs: instead the PR's own commits are examined normally. In
/// this way, the original type and size information from the PR is preserved.
#[allow(clippy::map_entry)]
pub async fn changes(auth: &Option<Auth>, repo: &Repo, baseref: FromTagBuf, headref: String) -> Result<Changes> {
  let mut all_commits = HashSet::new();
  let mut all_prs = HashMap::new();

  let mut discover_order = 0;
  let mut queue = VecDeque::new();
  let offset = FixedOffset::west_opt(0).expect("0 should be in bounds");
  let pr_zero = FullPr::lookup(
    repo,
    baseref,
    headref.clone(),
    0,
    "".into(),
    offset.timestamp_opt(Utc::now().timestamp(), 0).single().expect("utc/0 in bounds"),
    discover_order
  )?;
  discover_order += 1;
  queue.push_back(pr_zero.span().ok_or_else(|| bad!("Unable to get oid for seed ref \"{}\".", headref))?);
  all_prs.insert(pr_zero.number(), pr_zero);

  let github_info = match repo.github_info(auth) {
    Ok(github_info) => github_info,
    Err(_) => return Ok(Changes { groups: all_prs, commits: all_commits })
  };

  while let Some(span) = queue.pop_front() {
    let commit_list = commits_from_v4_api(&github_info, &span).await?;
    let commit_list: Vec<_> = commit_list
      .into_iter()
      .filter_map(|commit| {
        if all_commits.contains(commit.oid()) {
          return None;
        }

        let mut retain = true;
        let (oid, prs) = commit.extract();
        for pr in prs.merged_only() {
          let number = pr.number();
          if !all_prs.contains_key(&number) {
            let full_pr = match pr.lookup(repo, discover_order) {
              Ok(pr) => pr,
              Err(e) => return Some(Err(e))
            };
            discover_order += 1;
            if let Some(span) = full_pr.span() {
              queue.push_back(span);
            }
            all_prs.insert(number, full_pr);
          }
          let full_pr = all_prs.get_mut(&number).unwrap();

          if full_pr.best_guess() {
            full_pr.add_commit(CommitInfoBuf::guess(oid.clone()));
          } else if !full_pr.contains(&oid) {
            retain = false;
          }
        }

        if retain {
          Some(Ok(oid))
        } else {
          all_prs.get_mut(&span.number()).unwrap().add_exclude(&oid);
          None
        }
      })
      .collect::<Result<_>>()?;

    all_commits.extend(commit_list.into_iter());
  }

  Ok(Changes { commits: all_commits, groups: all_prs })
}

pub fn line_commits_head(repo: &Repo, base: FromTag) -> Result<Vec<CommitInfoBuf>> {
  repo.commits_to_head(base, false)?.map(|i| i?.buffer()).collect::<Result<_>>()
}

async fn commits_from_v4_api(github_info: &GithubInfo, span: &Span) -> Result<Vec<ApiCommit>> {
  let query = r#"query associatedPRs($since:GitTimestamp!, $sha:String!, $repo:String!, $owner:String!){
  repository(name:$repo, owner:$owner){
    commit:object(expression: $sha){
      ... on Commit {
        oid
        history(first:100, since:$since) {
          pageInfo {
            hasNextPage
            endCursor
          }
          nodes { ...commitResult }
        }
      }
    }
  }
}

fragment commitResult on Commit {
    oid
    associatedPullRequests(first:10) {
      edges {
        node {
          number
          title
          state
          headRefName
          baseRefOid
          closedAt
        }
      }
    }
    parents(first:10) {
      edges {
        node {
          oid
        }
      }
    }
}"#;

  let variables = format!(
    r#"{{ "sha": "{}", "since": "{}", "owner": "{}", "repo": "{}" }}"#,
    span.end(),
    time_to_datetime(span.since()).to_rfc3339(),
    github_info.owner_name(),
    github_info.repo_name()
  );

  let octo = Octocrab::builder();
  let token = github_info.token().clone();
  let octo = if let Some(token) = token { octo.personal_token(token) } else { octo };
  let octo = octo.build()?;
  let full_query = serde_json::json!({"query": &query, "variables": &variables});
  let changes: ChangesResponse = octo.post("/graphql", Some(&full_query)).await?;

  let changes = changes.data.repository.commit.history.nodes;
  let mut changes: HashMap<String, ApiCommit> = changes.into_iter().map(|c| (c.oid().to_string(), c)).collect();

  // Remove anything reachable by span.begin()
  let mut remqueue = VecDeque::new();
  remqueue.push_back(span.begin().tag().to_string());
  while let Some(rem) = remqueue.pop_front() {
    if let Some(commit) = changes.remove(&rem) {
      for edge in commit.parents.edges {
        remqueue.push_back(edge.node.oid.clone());
      }
    }
  }

  Ok(changes.into_values().collect())
}

pub struct Changes {
  commits: HashSet<String>,
  groups: HashMap<u32, FullPr>
}

impl Changes {
  pub fn commits(&self) -> &HashSet<String> { &self.commits }
  pub fn groups(&self) -> &HashMap<u32, FullPr> { &self.groups }
  pub fn into_groups(self) -> HashMap<u32, FullPr> { self.groups }
}

#[derive(Deserialize)]
struct ChangesResponse {
  data: Data
}

#[derive(Deserialize)]
struct Data {
  repository: RawRepository
}

#[derive(Deserialize)]
struct RawRepository {
  commit: TopCommit
}

#[derive(Deserialize)]
struct TopCommit {
  history: History
}

#[derive(Deserialize)]
struct History {
  nodes: Vec<ApiCommit>
}

#[derive(Deserialize)]
struct ApiCommit {
  oid: String,
  #[serde(rename = "associatedPullRequests")]
  associated_pull_requests: PrList,
  parents: ParentList
}

impl ApiCommit {
  fn extract(self) -> (String, PrList) { (self.oid, self.associated_pull_requests) }
  fn oid(&self) -> &str { &self.oid }
}

#[derive(Deserialize)]
struct ParentList {
  edges: Vec<ParentEdge>
}

#[derive(Deserialize)]
struct ParentEdge {
  node: ParentNode
}

#[derive(Deserialize)]
struct ParentNode {
  oid: String
}

#[derive(Deserialize)]
struct PrList {
  edges: Vec<PrEdge>
}

impl PrList {
  fn merged_only(self) -> impl Iterator<Item = PrEdgeNode> {
    self.edges.into_iter().map(|e| e.node).filter(|n| n.state() == "MERGED" || n.state() == "OPEN")
  }
}

#[derive(Deserialize)]
struct PrEdge {
  node: PrEdgeNode
}

#[derive(Deserialize)]
struct PrEdgeNode {
  number: u32,
  state: String,
  title: String,
  #[serde(rename = "headRefName")]
  head_ref_name: String,
  #[serde(rename = "baseRefOid")]
  base_ref_oid: String,
  #[serde(rename = "closedAt", deserialize_with = "deserialize_datetime")]
  closed_at: DateTime<FixedOffset>
}

impl PrEdgeNode {
  pub fn number(&self) -> u32 { self.number }
  pub fn state(&self) -> &str { &self.state }

  pub fn lookup(self, repo: &Repo, discover_order: usize) -> Result<FullPr> {
    FullPr::lookup(
      repo,
      FromTagBuf::new(self.base_ref_oid, false),
      self.head_ref_name,
      self.number,
      self.title,
      self.closed_at,
      discover_order
    )
  }
}

fn deserialize_datetime<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<DateTime<FixedOffset>, D::Error> {
  struct DateTimeVisitor;

  impl<'de> Visitor<'de> for DateTimeVisitor {
    type Value = DateTime<FixedOffset>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("an RFC 3339 datetime") }

    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
      if v.is_empty() || v.trim() == "null" {
        return self.visit_none();
      }
      DateTime::parse_from_rfc3339(v).map_err(|e| de::Error::custom(format!("Couldn't parse date {}: {:?}", v, e)))
    }

    fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
      let offset = FixedOffset::west_opt(0).expect("0 in bounds");
      Ok(offset.timestamp_opt(Utc::now().timestamp(), 0).single().expect("utc/0 in bounds"))
    }

    fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> { self.visit_none() }
  }

  desr.deserialize_any(DateTimeVisitor)
}
