//! Interactions with github API v4.

use crate::error::Result;
use crate::git::{FullPr, Span};
use chrono::{DateTime, FixedOffset, TimeZone};
use git2::{Repository, Time};
use github_gql::{client::Github, IntoGithubRequest};
use hyper::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use hyper::Request;
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};

/// Find all changes in a repo more cleverly than `git rev-parse begin..end` using the GitHub v4 API.
///
/// Our rationale is such: When GitHub squash-merges a PR, it creates a new commit on the base-ref branch. While
/// the commit is not a descendent of any commit on the PR, the commit still is associated with the source PR in
/// the GitHub API, and by default contains the PR number in its commit headline. Thus, it is associated with a
/// PR that it is not itself a part of, which is how we identify it.
///
/// We want to exclude such commits because these commits have lost some of the information contained in the
/// original PR: namely, what files are associated with what commit sizes. All the files altered in the commit
/// are associated with whatever size the single squash commit is, which may not be strictly correct.
///
/// Instead of including such commits, we want to include all of the commits from the associated PR (unless
/// those commits are themselves squash-merges, etc.)
///
/// To find all of the original commits, we first queue a "PR zero" that contains the naive `begin..end`. Then
/// for each queued PR, we examine each commit, and exclude it if: (a) the commit is associated with another PR,
/// and (b) that other PR's commits doesn't contain the original commit. We then queue that other PR, if
/// possible. Our result is a list of PRs, each of which has "base..head" rev-parse-able refs, and a list of
/// commits which should be excluded from them.
// all_prs.contains_key/insert w/ a side effect triggers a false positive.
#[allow(clippy::map_entry)]
pub fn changes(repo: &Repository, owner: String, repo_name: String, end: String, begin: String) -> Result<Changes> {
  let mut all_commits = HashSet::new();
  let mut all_prs = HashMap::new();

  let pr_zero = PrEdgeNode { number: 0, state: "MERGED".to_string(), head_ref_oid: end, base_ref_oid: begin };
  let pr_zero = pr_zero.lookup_full(repo)?;

  let mut queue = VecDeque::new();
  queue.push_back(pr_zero.span());
  all_prs.insert(pr_zero.number(), pr_zero);

  while let Some(span) = queue.pop_front() {
    let commit_list = commits_from_api(&owner, &repo_name, &span)?;
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
            let full_pr = match pr.lookup_full(repo) {
              Ok(pr) => pr,
              Err(e) => return Some(Err(e))
            };
            if !full_pr.best_guess() {
              queue.push_back(full_pr.span());
            }
            all_prs.insert(number, full_pr);
          }
          let full_pr = all_prs.get_mut(&number).unwrap();

          if full_pr.best_guess() {
            full_pr.add_commit(&oid);
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

  // TODO: remove non-orphans from commits ?
  // TODO: include files in commits ?

  Ok(Changes { commits: all_commits, groups: all_prs })
}

fn commits_from_api(owner: &str, repo: &str, span: &Span) -> Result<Vec<ApiCommit>> {
  // TODO : respect "hasNextPage" and endCursor by using history(after:)
  let query = r#"
query associatedPRs($since:GitTimestamp!, $sha:String!, $repo:String!, $owner:String!){
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
          state
          headRefOid
          baseRefOid
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
    owner,
    repo
  );

  // TODO: actual API token
  let token = "f517363ac4a9fc04df72aeccba4765fa73d719c6";
  let mut github = Github::new(token)?;
  let query = QueryVars::new(query.to_string(), variables);
  let (_headers, _status, resp) = github.run::<ChangesResponse, _>(&query)?;

  let changes = resp.ok_or_else(|| versio_error!("Couldn't find commits."))?;
  let changes = changes.data.repository.commit.history.nodes;
  let mut changes: HashMap<String, ApiCommit> = changes.into_iter().map(|c| (c.oid().to_string(), c)).collect();

  let mut remqueue = VecDeque::new();
  remqueue.push_back(span.begin().to_string());
  while let Some(rem) = remqueue.pop_front() {
    if let Some(commit) = changes.remove(&rem) {
      for edge in commit.parents.edges {
        remqueue.push_back(edge.node.oid.clone());
      }
    }
  }

  Ok(changes.into_iter().map(|(_, v)| v).collect())
}

fn time_to_datetime(time: &Time) -> DateTime<FixedOffset> {
  const MINUTES: i32 = 60;
  FixedOffset::east(time.offset_minutes() * MINUTES).timestamp(time.seconds(), 0)
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
    self.edges.into_iter().map(|e| e.node).filter(|n| n.state() == "MERGED")
  }
}

#[derive(Deserialize)]
struct PrEdge {
  node: PrEdgeNode
}

#[derive(Deserialize)]
pub struct PrEdgeNode {
  number: u32,
  state: String,
  #[serde(rename = "headRefOid")]
  head_ref_oid: String,
  #[serde(rename = "baseRefOid")]
  base_ref_oid: String
}

impl PrEdgeNode {
  pub fn number(&self) -> u32 { self.number }
  pub fn state(&self) -> &str { &self.state }

  pub fn lookup_full(self, repo: &Repository) -> Result<FullPr> {
    FullPr::lookup(repo, self.head_ref_oid, self.base_ref_oid, self.number)
  }
}

#[derive(Default)]
struct QueryVars {
  query: String,
  variables: String
}

impl QueryVars {
  pub fn new(query: String, variables: String) -> QueryVars { QueryVars { query, variables } }
}

impl IntoGithubRequest for QueryVars {
  fn into_github_req(&self, token: &str) -> github_gql::errors::Result<Request<hyper::Body>> {
    use github_gql::errors::ResultExt;

    // escaping new lines and quotation marks for json
    let query = escape(&self.query);
    let variables = escape(&self.variables);

    let mut q = String::from("{ \"query\": \"");
    q.push_str(&query);
    q.push_str("\", \"variables\": \"");
    q.push_str(&variables);
    q.push_str("\" }");
    let mut req = Request::builder()
      .method("POST")
      .uri("https://api.github.com/graphql")
      .body(q.into())
      .chain_err(|| "Unable to for URL to make the request")?;

    let token = String::from("token ") + token;
    {
      let headers = req.headers_mut();
      headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
      headers.insert(USER_AGENT, HeaderValue::from_static("github-rs"));
      headers.insert(AUTHORIZATION, HeaderValue::from_str(&token).chain_err(|| "token parse")?);
    }

    Ok(req)
  }
}

fn escape(val: &str) -> String {
  let mut escaped = val.to_string();
  escaped = escaped.replace("\n", "\\n");
  escaped = escaped.replace("\"", "\\\"");

  escaped
}

// fn deserialize_datetime<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<DateTime<FixedOffset>, D::Error> {
//   struct DateTimeVisitor;
//
//   impl<'de> Visitor<'de> for DateTimeVisitor {
//     type Value = DateTime<FixedOffset>;
//
//     fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("an RFC 3339 datetime") }
//
//     fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
//       DateTime::parse_from_rfc3339(v).map_err(|e| de::Error::custom(format!("Couldn't parse date {}: {:?}", v, e)))
//     }
//   }
//
//   desr.deserialize_str(DateTimeVisitor)
// }
