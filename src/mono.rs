use crate::config::Config;

pub struct Mono {
  current: Config<CurrentState>,
  next: StateWrite,
  last_commits: HashMap<ProjectId, String>,
  repo: Repo,
}

impl Mono {
  pub fn here() -> Result<Mono> { Mono::open(".") }

  pub fn open<P: AsRef<Path>>(dir: P) -> Result<Mono> {
    let repo = Repo::open(dir.as_ref())?;
    let old_tags = current.find_old_tags(&repo)?;
    let current = Config::from_state(CurrentState::open(dir.as_ref(), old_tags)?)?;
    let last_commits = find_last_commits

    Ok(Mono { current, repo })
  }

  // pub fn current_source(&self) -> &CurrentSource { self.current.source() }
  // pub fn current_config(&self) -> &Config<CurrentSource> { &self.current }
  // pub fn old_tags(&self) -> &OldTags { &self.old_tags }
  // pub fn repo(&self) -> &Repo { &self.repo }
  // pub fn pull(&self) -> Result<()> { self.repo().pull() }
  // pub fn is_configured(&self) -> Result<bool> { Config::has_config_file(self.current_source()) }

  pub fn set_by_id(&self, id: ProjectId, val: &str, new_tags: &mut NewTags, wrote: bool) -> Result<()> {
    let last_commits = self.find_last_commits()?;
    let proj = self.current_config().get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
    proj.set_value(self, val, last_commits.get(&id), new_tags, wrote)
  }

  pub fn forward_by_id(&self, id: ProjectId, val: &str, new_tags: &mut NewTags, wrote_something: bool) -> Result<()> {
    let last_commits = self.find_last_commits()?;
    let proj = self.current_config().get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
    proj.forward_value(val, last_commits.get(&id), new_tags, wrote_something)
  }

  pub fn set_by_name(&self, name: &str, val: &str, new_tags: &mut NewTags, wrote: bool) -> Result<()> {
    let curt_cfg = self.current_config();
    let id = curt_cfg.find_unique(name)?;
    self.set_by_id(id, val, new_tags, wrote)
  }

  pub fn changes(&self) -> Result<Changes> {
    let base = self.current_config().prev_tag().to_string();
    let head = self.repo().branch_name().to_string();
    changes(&self.repo, head, base)
  }

  pub fn keyed_files<'a>(&'a self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'a> {
    let changes = self.changes()?;
    let prs = changes.into_groups().into_iter().map(|(_, v)| v).filter(|pr| !pr.best_guess());

    let mut vec = Vec::new();
    for pr in prs {
      vec.push(pr_keyed_files(&self.repo, pr));
    }

    Ok(vec.into_iter().flatten())
  }

  pub fn diff(&self) -> Result<Analysis> {
    let prev_spec = self.current_config().prev_tag().to_string();
    let prev_config = Config::from_source(SliceSource::new(self.repo().slice(prev_spec))?)?;

    let curt_annotate = prev_config.annotate(&self.old_tags)?;
    let prev_annotate = self.current_config().annotate(&self.old_tags)?;

    Ok(analyze(prev_annotate, curt_annotate))
  }

  /* TODO: HERE: rejigger for Mono instead of Config */

  // pub fn check(&self) -> Result<()> {
  //   for project in &self.file.projects {
  //     project.check(&self.source)?;
  //   }
  //   Ok(())
  // }

  // pub fn get_mark_value(&self, id: ProjectId) -> Option<Result<String>> {
  //   self.get_project(id).map(|p| p.get_mark_value(&self.source))
  // }

  // pub fn show(&self, format: ShowFormat) -> Result<()> {
  //   let name_width = self.file.projects.iter().map(|p| p.name.len()).max().unwrap_or(0);

  //   for project in &self.file.projects {
  //     project.show(&self.source, name_width, &format)?;
  //   }
  //   Ok(())
  // }

  // pub fn show_id(&self, id: ProjectId, format: ShowFormat) -> Result<()> {
  //   let project = self.get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
  //   project.show(&self.source, 0, &format)
  // }

  // pub fn show_names(&self, name: &str, format: ShowFormat) -> Result<()> {
  //   let filter = |p: &&Project| p.name.contains(name);
  //   let name_width = self.file.projects.iter().filter(filter).map(|p| p.name.len()).max().unwrap_or(0);

  //   for project in self.file.projects.iter().filter(filter) {
  //     project.show(&self.source, name_width, &format)?;
  //   }
  //   Ok(())
  // }

  pub fn configure_plan(&self) -> Result<Plan> {
    let prev_spec = self.current_config().prev_tag().to_string();
    let head = self.repo().branch_name().to_string();
    let curt_config = self.current_config();
    let prev_config = Config::from_source(SliceSource::new(self.repo().slice(prev_spec.clone()))?)?;

    let mut plan = PlanConsider::new(prev_config, &curt_config);

    // Consider the grouped, unsquashed commits to determine project sizing and changelogs.
    for pr in changes(self.repo(), head, prev_spec)?.groups().values() {
      plan.consider_pr(pr)?;
      for commit in pr.included_commits() {
        plan.consider_commit(commit.clone())?;
        for file in commit.files() {
          plan.consider_file(file)?;
          plan.finish_file()?;
        }
        plan.finish_commit()?;
      }
      plan.finish_pr()?;
    }

    let last_commits = self.find_last_commits()?;
    plan.consider_last_commits(&last_commits)?;

    // Some projects might depend on other projects.
    plan.consider_deps()?;

    // Sort projects by earliest closed date, mark duplicate commits.
    plan.sort_and_dedup()?;

    plan.finish_plan()
  }

  /// Find the last covering commit ID, if any, for each current project.
  fn find_last_commits(&self) -> Result<HashMap<ProjectId, String>> {
    let prev_spec = self.current_config().prev_tag().to_string();
    let head = self.repo().branch_name().to_string();
    let curt_config = self.current_config();
    let prev_config = Config::from_source(SliceSource::new(self.repo().slice(prev_spec.clone()))?)?;

    let mut last_finder = LastCommitFinder::new(prev_config, &curt_config);

    // Consider the in-line commits to determine the last commit (if any) for each project.
    for commit in line_commits(self.repo(), head, prev_spec)? {
      last_finder.consider_line_commit(&commit)?;
      for file in commit.files() {
        last_finder.consider_line_file(file)?;
        last_finder.finish_line_file()?;
      }
      last_finder.finish_line_commit()?;
    }

    last_finder.finish_finder()
  }
}

fn pr_keyed_files<'a>(repo: &'a Repo, pr: FullPr) -> impl Iterator<Item = Result<(String, String)>> + 'a {
  let head_oid = match pr.head_oid() {
    Some(oid) => *oid,
    None => return E3::C(iter::empty())
  };

  let iter = repo.commits_between(pr.base_oid(), head_oid).map(move |cmts| {
    cmts
      .filter_map(move |cmt| match cmt {
        Ok(cmt) => {
          if pr.has_exclude(&cmt.id()) {
            None
          } else {
            match cmt.files() {
              Ok(files) => {
                let kind = cmt.kind();
                Some(E2::A(files.map(move |f| Ok((kind.clone(), f)))))
              }
              Err(e) => Some(E2::B(iter::once(Err(e))))
            }
          }
        }
        Err(e) => Some(E2::B(iter::once(Err(e))))
      })
      .flatten()
  });

  match iter {
    Ok(iter) => E3::A(iter),
    Err(e) => E3::B(iter::once(Err(e)))
  }
}
