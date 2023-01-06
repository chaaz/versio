//! Template and changelog management for Versio.

use crate::bail;
use crate::errors::Result;
use crate::mono::{Changelog, ChangelogEntry};
use crate::output::ProjLine;
use chrono::prelude::Utc;
use hyper::Client;
use liquid::ParserBuilder;
use path_slash::PathBufExt;
use std::path::{Path, PathBuf};

/// Extract everything in an old changelog between the `BEGIN CONTENT` and `END CONTENT` lines.
pub fn extract_old_content(path: &Path) -> Result<String> {
  if !path.exists() {
    return Ok("".into());
  }

  let full_content = std::fs::read_to_string(path)?;
  let content = full_content
    .split('\n')
    .skip_while(|l| !l.contains("### VERSIO BEGIN CONTENT ###"))
    .skip(1)
    .take_while(|l| !l.contains("### VERSIO END CONTENT ###"))
    .collect::<Vec<_>>()
    .join("\n");
  Ok(content)
}

pub fn construct_changelog_html(
  cl: &Changelog, proj: ProjLine, new_vers: &str, old_content: String, tmpl: String
) -> Result<String> {
  let tmpl = ParserBuilder::with_stdlib().build()?.parse(&tmpl)?;
  let nowymd = Utc::now().format("%Y-%m-%d").to_string();

  let pr_count = cl
    .entries()
    .iter()
    .filter(|entry| match entry {
      ChangelogEntry::Pr(pr, _) => pr.commits().iter().any(|c| c.included()),
      _ => false
    })
    .count();

  let mut prs = Vec::new();
  let mut dps = Vec::new();

  for entry in cl.entries() {
    match entry {
      ChangelogEntry::Pr(pr, size) => {
        if !pr.commits().iter().any(|c| c.included()) {
          continue;
        }

        let mut commits = Vec::new();
        for c in pr.commits().iter().filter(|c| c.included()) {
          commits.push(liquid::object!({
            "href": c.url().as_deref().unwrap_or(""),
            "link": c.url().is_some(),
            "shorthash": c.oid()[.. 7].to_string(),
            "size": c.size().to_string(),
            "summary": c.summary(),
            "message": c.message().trim()
          }));
        }

        let pr_name = if pr.number() == 0 {
          if pr_count == 1 {
            "Commits".to_string()
          } else {
            "Other commits".to_string()
          }
        } else {
          format!("PR {}", pr.number())
        };

        prs.push(liquid::object!({
          "title": pr.title(),
          "name": pr_name,
          "size": size.to_string(),
          "href": pr.url().as_deref().unwrap_or(""),
          "link": pr.number() > 0 && pr.url().is_some(),
          "commits": commits
        }));
      }
      ChangelogEntry::Dep(proj_id, name) => {
        dps.push(liquid::object!({
          "id": proj_id.to_string(),
          "name": name
        }));
      }
    }
  }

  let globals = liquid::object!({
    "project": {
      "id": proj.id.to_string(),
      "name": proj.name,
      "tag_prefix": proj.tag_prefix.unwrap_or_default(),
      "tag_prefix_separator": proj.tag_prefix_separator,
      "version": proj.version,
      "full_version": proj.full_version.unwrap_or_default(),
      "root": proj.root.unwrap_or_default(),
    },
    "release": {
      "date": nowymd,
      "prs": prs,
      "deps": dps,
      "version": new_vers
    },
    "old_content": old_content,
    "content_marker": format!("CONTENT {}", nowymd)
  });

  Ok(tmpl.render(&globals)?)
}

pub async fn read_template(tmpl_url: &str, base_path: Option<&Path>, forward_slash: bool) -> Result<String> {
  let parts: Vec<_> = tmpl_url.splitn(2, ':').collect();
  if parts.len() > 1 {
    match parts[0] {
      "builtin" => match parts[1] {
        "html" => Ok(include_str!("tmpl/changelog.liquid").to_string()),
        "json" => Ok(include_str!("tmpl/json.liquid").to_string()),
        _ => bail!("Unknown builtin template: {}", parts[1])
      },
      "file" => {
        let path = if forward_slash { PathBuf::from_slash(parts[1]) } else { PathBuf::from(parts[1]) };
        match base_path {
          Some(base_path) => Ok(std::fs::read_to_string(base_path.join(path))?),
          None => Ok(std::fs::read_to_string(path)?)
        }
      }
      "http" | "https" => {
        let resp = Client::new().get(tmpl_url.parse()?).await?;
        if !resp.status().is_success() {
          bail!("Unsuccessful request to {}: {}", tmpl_url, resp.status().as_u16());
        }

        let body = hyper::body::to_bytes(resp.into_body()).await?;
        Ok(String::from_utf8(body.to_vec())?)
      }
      _ => bail!("Unrecognized template protocol: {}", parts[0])
    }
  } else {
    bail!("Template URL has no protocol: {}", tmpl_url);
  }
}
