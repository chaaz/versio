//! The command-line options for the executable.

use clap::error::ErrorKind;
use clap::{ArgGroup, CommandFactory, Parser, Subcommand, ValueEnum};
use versio::commands::*;
use versio::errors::Result;
use versio::init::init;
use versio::vcs::{VcsLevel, VcsRange};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Cli {
  /// The VCS level
  #[arg(short = 'l', long, value_enum)]
  vcs_level: Option<VcsLevelArg>,

  /// The minimum VCS level
  #[arg(short = 'm', long, value_enum)]
  vcs_level_min: Option<VcsLevelBound>,

  /// The maximum VCS level
  #[arg(short = 'x', long, value_enum)]
  vcs_level_max: Option<VcsLevelBound>,

  /// Ignore local repo changes
  #[arg(short = 'c', long)]
  no_current: bool,

  #[command(subcommand)]
  command: Commands
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, ValueEnum)]
enum VcsLevelArg {
  Auto,
  Max,
  None,
  Local,
  Remote,
  Smart
}

impl VcsLevelArg {
  fn to_vcs_range(self) -> Option<VcsRange> {
    match self {
      Self::Auto => None,
      Self::Max => Some(VcsRange::full()),
      Self::None => Some(VcsRange::exact(VcsLevel::None)),
      Self::Local => Some(VcsRange::exact(VcsLevel::Local)),
      Self::Remote => Some(VcsRange::exact(VcsLevel::Remote)),
      Self::Smart => Some(VcsRange::exact(VcsLevel::Smart))
    }
  }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, ValueEnum)]
enum VcsLevelBound {
  None,
  Local,
  Remote,
  Smart
}

impl VcsLevelBound {
  fn to_vcs_level(self) -> VcsLevel {
    match self {
      Self::None => VcsLevel::None,
      Self::Local => VcsLevel::Local,
      Self::Remote => VcsLevel::Remote,
      Self::Smart => VcsLevel::Smart
    }
  }
}

#[derive(Debug, Subcommand)]
enum Commands {
  /// Check current config
  Check {},

  /// Show all versions
  Show {
    /// Whether to show prev version
    #[arg(short, long)]
    prev: bool,

    /// Wide output shows IDs
    #[arg(short, long)]
    wide: bool
  },

  /// Get one or more versions
  #[command(group(ArgGroup::new("ident").args(["name", "id", "exact"]),))]
  Get {
    /// Whether to show prev versions
    #[arg(short, long)]
    prev: bool,

    /// Only show the version number
    #[arg(short, long)]
    version_only: bool,

    /// Wide output shows IDs
    #[arg(short, long)]
    wide: bool,

    /// The name to get.
    #[arg(short, long)]
    name: Option<String>,

    /// The exact name to get.
    #[arg(short, long)]
    exact: Option<String>,

    /// The ID to get.
    #[arg(short, long)]
    id: Option<u32>
  },

  /// Set a version.
  #[command(group(ArgGroup::new("ident").args(["name", "id", "exact"]),))]
  Set {
    /// The name to set.
    #[arg(short, long)]
    name: Option<String>,

    /// The ID to set.
    #[arg(short, long)]
    id: Option<u32>,

    /// The exact name to set.
    #[arg(short, long)]
    exact: Option<String>,

    /// The new value
    #[arg(short, long)]
    value: String
  },

  /// View changes from previous
  Diff {},

  /// Stream changed files
  Files {},

  /// Find versions that need to change
  Plan {
    /// The changelog template to format with
    #[arg(short, long)]
    template: Option<String>,

    /// Plan only a single project
    #[arg(short, long)]
    id: Option<u32>
  },

  /// Change and commit version numbers
  #[command(group(ArgGroup::new("partial").args(["resume", "abort"]),))]
  Release {
    /// Also show unchanged versions
    #[arg(short = 'a', long)]
    show_all: bool,

    /// Pause the release
    #[arg(short, long, value_enum)]
    pause: Option<PauseStage>,

    /// Resume after pausing
    #[arg(long)]
    resume: bool,

    /// Abort after pausing
    #[arg(long)]
    abort: bool,

    #[arg(short, long)]
    dry_run: bool,

    #[arg(short, long)]
    changelog_only: bool,

    #[arg(short, long)]
    lock_tags: bool
  },

  /// Print true changes
  Changes {},

  /// Search for projects and write a config
  Init {
    /// Max descent to search
    #[arg(short = 'd', long, default_value_t = 5)]
    max_depth: u16
  },

  /// Print info about projects
  Info {
    /// Info on a project ID
    #[arg(short, long)]
    id: Vec<u32>,

    /// Info on a project name
    #[arg(short, long)]
    name: Vec<String>,

    /// Info on an exact project name
    #[arg(short, long)]
    exact: Vec<String>,

    /// Info on a labeled project
    #[arg(short, long)]
    label: Vec<String>,

    /// Info on all projects
    #[arg(short, long)]
    all: bool,

    /// Show all fields
    #[arg(short = 'A', long)]
    show_all: bool,

    /// Show the project(s) root
    #[arg(short = 'R', long)]
    show_root: bool,

    /// Show the project(s) name
    #[arg(short = 'N', long)]
    show_name: bool,

    /// Show the project(s) ID
    #[arg(short = 'I', long)]
    show_id: bool,

    /// Show the project(s) full version with tag prefix
    #[arg(short = 'F', long)]
    show_full_version: bool,

    /// Show the project(s) version
    #[arg(short = 'V', long)]
    show_version: bool,

    /// Show the project(s) tag prefix
    #[arg(short = 'T', long)]
    show_tag_prefix: bool
  },

  /// Output a changelog template
  Template {
    /// The changelog template to output
    #[arg(short, long)]
    template: String
  },

  /// Output a JSON schema for the config file
  Schema {}
}

impl Commands {
  fn requires_sanity(&self) -> bool {
    match self {
      Self::Release { abort, resume, .. } => !*abort && !*resume,
      _ => true
    }
  }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, ValueEnum)]
enum PauseStage {
  Commit
}

pub async fn execute(early_info: &EarlyInfo) -> Result<()> {
  let id_required = early_info.project_count() != 1;
  let cli = Cli::parse();
  verify_cli(&cli, id_required)?;

  if cli.command.requires_sanity() {
    sanity_check()?;
  }

  let pref_vcs = parse_vcs(&cli);
  let no_current = cli.no_current;

  match &cli.command {
    Commands::Check {} => check(pref_vcs, no_current)?,
    Commands::Get { prev, version_only, wide, name, exact, id } => {
      let name_match = NameMatch::from(name, exact);
      get(pref_vcs, *wide, *version_only, *prev, id.as_ref(), &name_match, no_current)?
    }
    Commands::Show { prev, wide } => show(pref_vcs, *wide, *prev, no_current)?,
    Commands::Set { name, exact, id, value } => {
      let name_match = NameMatch::from(name, exact);
      set(pref_vcs, id.as_ref(), &name_match, value)?
    }
    Commands::Diff {} => diff(pref_vcs, no_current)?,
    Commands::Files {} => files(pref_vcs, no_current).await?,
    Commands::Changes {} => changes(pref_vcs, no_current).await?,
    Commands::Plan { template, id } => plan(early_info, pref_vcs, id.as_ref(), template.as_deref(), no_current).await?,
    Commands::Release { abort: a, .. } if *a => abort()?,
    Commands::Release { resume: r, .. } if *r => resume(pref_vcs)?,
    Commands::Release { show_all, pause, dry_run, changelog_only, lock_tags, .. } => {
      let dry = if *dry_run {
        Engagement::Dry
      } else if *changelog_only {
        Engagement::Changelog
      } else {
        Engagement::Full
      };

      release(pref_vcs, *show_all, &dry, *lock_tags, pause.is_some()).await?
    }
    Commands::Init { max_depth } => init(*max_depth)?,
    Commands::Info {
      id,
      name,
      exact,
      label,
      all,
      show_all,
      show_root,
      show_name,
      show_id,
      show_full_version,
      show_version,
      show_tag_prefix
    } => {
      let show = InfoShow::new()
        .pick_all(*all)
        .show_name(*show_name || *show_all)
        .show_root(*show_root || *show_all)
        .show_id(*show_id || *show_all)
        .show_full_version(*show_full_version || *show_all)
        .show_version(*show_version || *show_all)
        .show_tag_prefix(*show_tag_prefix || *show_all);

      info(pref_vcs, id, name, exact, label, show, no_current)?
    }
    Commands::Template { template: t } => template(early_info, t).await?,
    Commands::Schema {} => schema()?
  }

  Ok(())
}

fn verify_cli(cli: &Cli, id_required: bool) -> Result<()> {
  if cli.vcs_level.is_some() && (cli.vcs_level_min.is_some() || cli.vcs_level_max.is_some()) {
    let mut cmd = Cli::command();
    cmd.error(ErrorKind::ValueValidation, "Cannot use vcs-level-min or -max when vcs-level is set.").exit();
  }

  if cli.vcs_level_max.is_some() != cli.vcs_level_min.is_some() {
    let mut cmd = Cli::command();
    cmd.error(ErrorKind::ValueValidation, "vcs-level-min and vcs-level-max must both be set, or neither.").exit();
  }

  if let Commands::Get { prev, name, id, exact, .. } = &cli.command {
    let is_idented = name.is_some() || id.is_some() || exact.is_some();
    if *prev && !is_idented {
      let mut cmd = Cli::command();
      cmd.error(ErrorKind::ValueValidation, "Unable to use `prev` without a name or ID.").exit();
    }

    if !is_idented && id_required {
      let mut cmd = Cli::command();
      cmd.error(ErrorKind::ValueValidation, "Name or ID required for multi-project config.").exit();
    }
  }

  if let Commands::Set { name, id, exact, .. } = &cli.command {
    let is_idented = name.is_some() || id.is_some() || exact.is_some();
    if !is_idented && id_required {
      let mut cmd = Cli::command();
      cmd.error(ErrorKind::ValueValidation, "Name or ID required for multi-project config.").exit();
    }
  }

  if let Commands::Plan { id, template } = &cli.command {
    if template.is_some() && id.is_none() && id_required {
      let mut cmd = Cli::command();
      cmd.error(ErrorKind::ValueValidation, "Choose an ID for template plan.").exit();
    }
  }

  if let Commands::Release { dry_run, changelog_only, lock_tags, pause, resume, abort, .. } = &cli.command {
    if *dry_run && (pause.is_some() || *resume || *abort || *changelog_only) {
      let mut cmd = Cli::command();
      cmd
        .error(ErrorKind::ValueValidation, "dry-run can't be used with pause, resume, abort, or changelog-only")
        .exit();
    }

    if *changelog_only && (pause.is_some() || *resume || *abort) {
      let mut cmd = Cli::command();
      cmd.error(ErrorKind::ValueValidation, "changelog-only can't be used with pause, resume, or abort").exit();
    }

    if *lock_tags && (pause.is_some() || *resume || *abort) {
      let mut cmd = Cli::command();
      cmd.error(ErrorKind::ValueValidation, "lock-tags can't be used with pause, resume, or abort").exit();
    }
  }

  Ok(())
}

fn parse_vcs(cli: &Cli) -> Option<VcsRange> {
  if let Some(vcs_level) = &cli.vcs_level {
    vcs_level.to_vcs_range()
  } else if let Some(vcs_min) = &cli.vcs_level_min {
    let vcs_max = cli.vcs_level_max.as_ref().unwrap();
    Some(VcsRange::new(vcs_min.to_vcs_level(), vcs_max.to_vcs_level()))
  } else {
    None
  }
}
