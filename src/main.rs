use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use filterfs::filter::Filter;
use fuser::{Config, MountOption};

use filterfs::filterfs::FilterFS;
use filterfs::pattern::PatternRule;
use nix::unistd::{ForkResult, fork};

/// # FilterFS
///
/// A FUSE file system to mirror another directory, but filter what is shown.
#[derive(Parser)]
struct Args {
    /// Underlying source directory
    source: PathBuf,

    /// Mount point
    mountpoint: PathBuf,

    /// Run in foreground
    #[arg(short, long)]
    foreground: bool,

    /// FUSE-style mount options
    /// i.e. `-o 'inlc=*.so,inlc=*.TAG,dexcl=*-*'`
    /// will yield includes for both .so and .TAG, and in addition exclude all dirs with '-' in
    /// their names. The order of the application of rules is the order given. A file must match at
    /// least one include and no exclude to be shown. If no includes are given, only excludes are
    /// considered for display.
    ///
    /// Supported options:
    ///
    ///   - incl=glob - include file matching glob
    ///
    ///   - excl=glob - exclude file matching glob
    ///
    ///   - dincl=glob - include dir matching glob
    ///
    ///   - dexcl=glob - exclude dir matching glob
    ///
    ///   - prune=n - how deep to recursively search to see if dir is empty. Default is 0
    ///
    ///   - allow_other
    #[arg(short = 'o')]
    options: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if !args.foreground
        && let ForkResult::Parent { .. } = unsafe { fork().unwrap() }
    {
        return Ok(());
    }

    let mut file_rules = Vec::new();
    let mut dir_rules = Vec::new();

    let mut allow_other = false;
    let mut prune_depth = 0;
    env_logger::init();

    if let Some(options) = args.options {
        filterfs::args::parse_options(options, |key, value| {
            let value = value.ok_or_else(|| format!("{} needs a value!", key));
            let glob_fail = {
                let v2 = value.clone();
                |e| {
                    format!(
                        "unable to parse glob for '{}': '{}': {}",
                        key,
                        v2.unwrap(),
                        e
                    )
                }
            };
            match key {
                "incl" => {
                    file_rules.push(PatternRule::new_include(value?).map_err(glob_fail)?);
                }
                "excl" => {
                    file_rules.push(PatternRule::new_exclude(value?).map_err(glob_fail)?);
                }
                "dincl" => {
                    dir_rules.push(PatternRule::new_include(value?).map_err(glob_fail)?);
                }
                "dexcl" => {
                    dir_rules.push(PatternRule::new_exclude(value?).map_err(glob_fail)?);
                }
                "prune" => {
                    prune_depth = value
                        .clone()?
                        .parse()
                        .map_err(|e| format!("unable to num: '{}': {}", value.unwrap(), e))?;
                }
                "allow_other" => {
                    allow_other = true;
                }
                "rw" => {
                    eprintln!("RW unsupported, mounting as Read-Only filesystem!");
                }
                opt => {
                    eprintln!("unknown option: {:?}", opt);
                }
            }
            Ok(())
        })?;
    }

    let filesys = FilterFS::new(
        &args.source,
        prune_depth,
        Filter::new(&file_rules, &dir_rules),
    );
    let mut options = Config::default();
    options.mount_options = vec![
        MountOption::FSName("filterfs".to_string()),
        MountOption::DefaultPermissions,
    ];
    if allow_other {
        options.acl = fuser::SessionACL::All;
    }

    fuser::mount2(filesys, args.mountpoint, &options)?;

    Ok(())
}
