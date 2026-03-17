use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use fuser::{Config, MountOption};
use log::debug;

use filterfs::filterfs::FilterFS;
use filterfs::pattern::PatternRule;

/// FilterFS
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
    /// i.e. -o 'inlc=*.so,inlc=*.TAG,dexcl=*-*'
    /// will yield includes for both .so and .TAG, and in addition exclude all dirs with '-' in
    /// their names. The order of the application of rules is the order given. A file must match at
    /// least one include and no exclude to be shown. If no includes are given, only excludes are
    /// considered for display.
    /// Supported options:
    ///   - incl=glob
    ///     - include file matching glob
    ///   - excl=glob
    ///     - exclude file matching glob
    ///   - dincl=glob
    ///     - include dir matching glob
    ///   - dexcl=glob
    ///     - exclude dir matching glob
    ///   - prune=n
    ///     - how deep to recursively search to see if dir is empty.
    ///       default is 0
    #[arg(short = 'o')]
    options: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let mut file_incl = Vec::new();
    let mut file_excl = Vec::new();
    let mut dir_incl = Vec::new();
    let mut dir_excl = Vec::new();
    let mut allow_other = false;
    let mut prune_depth = 0;
    env_logger::init();

    if let Some(options) = args.options {
        for option in options.split(',') {
            let mut option = option.split('=');
            match option.next() {
                Some("incl") => {
                    let glob = option.next().unwrap();
                    file_incl.push(PatternRule::new_include(glob)?);
                    debug!("adding file include: '{}'", glob);
                }
                Some("excl") => {
                    let glob = option.next().unwrap();
                    file_excl.push(PatternRule::new_exclude(glob)?);
                    debug!("adding file exclude: '{}'", glob);
                }
                Some("dincl") => {
                    let glob = option.next().unwrap();
                    dir_incl.push(PatternRule::new_include(glob)?);
                    debug!("adding dir include: '{}'", glob);
                }
                Some("dexcl") => {
                    let glob = option.next().unwrap();
                    dir_excl.push(PatternRule::new_exclude(glob)?);
                    debug!("adding dir exclude: '{}'", glob);
                }
                Some("prune") => {
                    prune_depth = option.next().and_then(|v| v.parse().ok()).unwrap();
                }
                Some("allow_other") => {
                    allow_other = true;
                }
                opt => {
                    eprintln!("unknown option: {:?}", opt);
                }
            }
        }
    }

    let filesys = FilterFS::new(
        &args.source,
        prune_depth,
        file_incl,
        file_excl,
        dir_incl,
        dir_excl,
    );
    let mut options = Config::default();
    options.mount_options = vec![MountOption::FSName("filterfs".to_string())];
    if allow_other {
        options.acl = fuser::SessionACL::All;
    }

    Ok(fuser::mount2(filesys, args.mountpoint, &options)?)
}
