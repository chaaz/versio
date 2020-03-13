//! Interactions with github API v4.

use crate::error::Result;
use chrono::{DateTime, FixedOffset, TimeZone};
use git2::Time;
use github_gql::{client::Github, IntoGithubRequest};
use hyper::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use hyper::Request;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

pub fn changes(owner: String, repo: String, branch: String, since: Time, exclude: String) -> Result<()> {
  // TODO : respect hasNextPage and endCursor by using history(after:)
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
    abbreviatedOid
    messageHeadline
    messageHeadlineHTML
    messageBody
    messageBodyHTML
    author {
      user {
        login
      }
    }
    committedDate
    associatedPullRequests(first:10) {
      edges {
        node {
          title
          number
          body
          bodyHTML
          headRefName
          headRefOid
          headRef {
            id
            name
            target {
              abbreviatedOid
            }
          }
          baseRefName
          baseRefOid
          baseRef {
            id
            name
            target {
              abbreviatedOid
            }
          }
        }
      }
    }
}"#;

  const MINUTES: i32 = 60;
  let since: DateTime<FixedOffset> = FixedOffset::east(since.offset_minutes() * MINUTES).timestamp(since.seconds(), 0);

  let variables = format!(
    r#"{{ "sha": "{}", "since": "{}", "owner": "{}", "repo": "{}" }}"#,
    branch,
    since.to_rfc3339(),
    owner,
    repo
  );

  // TODO: actual API token
  let token = "f517363ac4a9fc04df72aeccba4765fa73d719c6";
  let mut github = Github::new(token)?;
  let query = QueryVars::new(query.to_string(), variables);
  let (headers, status, changes) = github.run::<ChangesResponse, _>(&query)?;

  let changes = changes.unwrap();
  let mut changes = changes.data.repository.commit.history.changes;
  changes.strip_exclude(&exclude);

  println!("headers: {:?}", headers);
  println!("status: {:?}", status);

  if !changes.pull_requests.is_empty() {
    println!("\nPRs:");
  }
  for pr in &changes.pull_requests {
    println!("- number: {}", pr.number);
    println!("  title: {}", pr.title);
    println!("  body: {}", pr.body);
    println!("  body (html): {}", pr.body_html);
    println!("  base oid: {}", pr.base_ref_oid);
    println!("  head oid: {}", pr.head_ref_oid);
    println!("  commits:");
    for commit in &pr.commits {
      println!("  - oid: {}", commit.abbreviated_oid);
      println!("    headline: {}", commit.message_headline);
      println!("    headline (html): {}", commit.message_headline_html);
      println!("    body: {}", commit.message_body);
      println!("    body (html): {}", commit.message_body_html);
      println!("    login: {}", commit.login.as_ref().map(|s| s.as_str()).unwrap_or("<none>"));
      println!("    committed: {}", commit.committed_date.to_rfc3339());
    }
  }

  if !changes.orphan_commits.is_empty() {
    println!("\norphan_commits:");
  }
  for commit in &changes.orphan_commits {
    println!("- oid: {}", commit.abbreviated_oid);
    println!("  headline: {}", commit.message_headline);
    println!("  headline (html): {}", commit.message_headline_html);
    println!("  body: {}", commit.message_body);
    println!("  body (html): {}", commit.message_body_html);
    println!("  login: {}", commit.login.as_ref().map(|s| s.as_str()).unwrap_or("<none>"));
    println!("  committed: {}", commit.committed_date.to_rfc3339());
  }

  Ok(())
}

#[derive(Deserialize)]
struct ChangesResponse {
  data: Data
}

#[derive(Deserialize)]
struct Data {
  repository: Repository
}

#[derive(Deserialize)]
struct Repository {
  commit: TopCommit
}

#[derive(Deserialize)]
struct TopCommit {
  history: History
}

#[derive(Deserialize)]
struct History {
  #[serde(rename = "nodes")]
  changes: Changes
}

struct Changes {
  orphan_commits: Vec<CommitInfo>,
  pull_requests: Vec<PrInfo>
}

impl Changes {
  fn strip_exclude(&mut self, exclude: &str) {
    self.orphan_commits.retain(|cmt| !cmt.oid.starts_with(exclude));
    for pr in &mut self.pull_requests {
      pr.commits.retain(|cmt| !cmt.oid.starts_with(exclude));
    }
    self.pull_requests.retain(|pr| !pr.commits.is_empty())
  }
}

impl<'de> Deserialize<'de> for Changes {
  fn deserialize<D: Deserializer<'de>>(desr: D) -> std::result::Result<Changes, D::Error> {
    struct ChangesVisitor;

    impl<'de> Visitor<'de> for ChangesVisitor {
      type Value = Changes;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a commit") }

      fn visit_seq<V>(self, mut seq: V) -> std::result::Result<Self::Value, V::Error>
      where
        V: SeqAccess<'de>
      {
        let mut orphan_commits = Vec::new();
        let mut pull_requests = HashMap::new();

        while let Some(commit) = seq.next_element::<RawCommit>()? {
          let commit_info = CommitInfo {
            oid: commit.oid,
            abbreviated_oid: commit.abbreviated_oid,
            login: commit.author.user.map(|user| user.login),
            committed_date: commit.committed_date,
            message_body: commit.message_body,
            message_body_html: commit.message_body_html,
            message_headline: commit.message_headline,
            message_headline_html: commit.message_headline_html
          };

          if commit.associated_pull_requests.edges.is_empty() {
            orphan_commits.push(commit_info);
          } else {
            for pr in commit.associated_pull_requests.edges {
              let pr = pr.node;
              let pr_info = pull_requests.entry(pr.number).or_insert(PrInfo {
                number: pr.number,
                title: pr.title,
                body: pr.body,
                body_html: pr.body_html,
                commits: Vec::new(),
                head_ref_oid: pr.head_ref_oid,
                base_ref_oid: pr.base_ref_oid
              });
              pr_info.commits.push(commit_info.clone());
            }
          }
        }

        let mut pull_requests: Vec<_> = pull_requests.drain().map(|(_, v)| v).collect();
        pull_requests.sort_by_key(|pr| pr.number);

        Ok(Changes { orphan_commits, pull_requests })
      }
    }

    desr.deserialize_seq(ChangesVisitor)
  }
}

#[derive(Clone)]
struct CommitInfo {
  oid: String,
  abbreviated_oid: String,
  message_headline: String,
  message_headline_html: String,
  message_body: String,
  message_body_html: String,
  login: Option<String>,
  committed_date: DateTime<FixedOffset>
}

struct PrInfo {
  number: u32,
  title: String,
  body: String,
  body_html: String,
  commits: Vec<CommitInfo>,
  head_ref_oid: String,
  base_ref_oid: String
}

#[derive(Deserialize)]
struct RawCommit {
  #[serde(rename = "abbreviatedOid")]
  abbreviated_oid: String,
  #[serde(rename = "associatedPullRequests")]
  associated_pull_requests: PrList,
  author: Author,
  #[serde(deserialize_with = "deserialize_datetime", rename = "committedDate")]
  committed_date: DateTime<FixedOffset>,
  #[serde(rename = "messageBody")]
  message_body: String,
  #[serde(rename = "messageBodyHTML")]
  message_body_html: String,
  #[serde(rename = "messageHeadline")]
  message_headline: String,
  #[serde(rename = "messageHeadlineHTML")]
  message_headline_html: String,
  oid: String
}

#[derive(Deserialize)]
struct PrList {
  edges: Vec<PrEdge>
}

#[derive(Deserialize)]
struct PrEdge {
  node: PrEdgeNode
}

#[derive(Deserialize)]
struct PrEdgeNode {
  number: u32,
  body: String,
  #[serde(rename = "bodyHTML")]
  body_html: String,
  title: String,
  #[serde(rename = "headRef")]
  _head_ref: Option<Ref>,
  #[serde(rename = "headRefName")]
  _head_ref_name: String,
  #[serde(rename = "headRefOid")]
  head_ref_oid: String,
  #[serde(rename = "baseRef")]
  _base_ref: Option<Ref>,
  #[serde(rename = "baseRefName")]
  _base_ref_name: String,
  #[serde(rename = "baseRefOid")]
  base_ref_oid: String
}

#[derive(Deserialize)]
struct Ref {
  #[serde(rename = "id")]
  _id: String,
  #[serde(rename = "name")]
  _name: String,
  #[serde(rename = "target")]
  _target: RefTarget
}

#[derive(Deserialize)]
struct RefTarget {
  #[serde(rename = "abbreviatedOid")]
  _abbreviated_oid: String
}

#[derive(Deserialize)]
struct Author {
  user: Option<User>
}

#[derive(Deserialize)]
struct User {
  login: String
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

fn deserialize_datetime<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<DateTime<FixedOffset>, D::Error> {
  struct DateTimeVisitor;

  impl<'de> Visitor<'de> for DateTimeVisitor {
    type Value = DateTime<FixedOffset>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("an RFC 3339 datetime") }

    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
      DateTime::parse_from_rfc3339(v).map_err(|e| de::Error::custom(format!("Couldn't parse date {}: {:?}", v, e)))
    }
  }

  desr.deserialize_str(DateTimeVisitor)
}
