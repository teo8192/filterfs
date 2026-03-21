use std::error::Error;
use std::path::PathBuf;

use clap::Parser;
use filterfs::filter::Filter;
use fuser::{Config, MountOption};
use log::debug;

use filterfs::filterfs::FilterFS;
use filterfs::pattern::PatternRule;

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

fn parse_options<F>(options: String, mut callback: F) -> Result<(), String>
where
    F: FnMut(&str, Option<&str>) -> Result<(), String>,
{
    for option in options.split(',') {
        let mut option = option.splitn(2, '=');
        if let Some(opt) = option.next() && !opt.is_empty() {
            callback(opt, option.next())?;
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if !args.foreground {
        match unsafe { nix::unistd::fork().unwrap() } {
            nix::unistd::ForkResult::Parent { .. } => return Ok(()),
            nix::unistd::ForkResult::Child => {}
        }
    }

    let mut file_rules = Vec::new();
    let mut dir_rules = Vec::new();

    let mut allow_other = false;
    let mut prune_depth = 0;
    env_logger::init();

    if let Some(options) = args.options {
        parse_options(options, |key, value| {
            let value = || value.ok_or_else(|| format!("{} needs a value!", key));
            let glob_fail = |e| format!("unable to parse glob: '{}': {}", value().unwrap(), e);
            match key {
                "incl" => {
                    let value = value()?;
                    file_rules.push(PatternRule::new_include(value).map_err(glob_fail)?);
                    debug!("adding file include: '{}'", value);
                }
                "excl" => {
                    let value = value()?;
                    file_rules.push(PatternRule::new_exclude(value).map_err(glob_fail)?);
                    debug!("adding file exclude: '{}'", value);
                }
                "dincl" => {
                    let value = value()?;
                    dir_rules.push(PatternRule::new_include(value).map_err(glob_fail)?);
                    debug!("adding dir include: '{}'", value);
                }
                "dexcl" => {
                    let value = value()?;
                    dir_rules.push(PatternRule::new_exclude(value).map_err(glob_fail)?);
                    debug!("adding dir exclude: '{}'", value);
                }
                "prune" => {
                    let value = value()?;
                    prune_depth = value
                        .parse()
                        .map_err(|e| format!("unable to num: '{}': {}", value, e))?;
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

    let filter = Filter::new(&file_rules, &dir_rules);

    let filesys = FilterFS::new(
        &args.source,
        prune_depth,
        filter,
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::parse_options;

    #[test]
    fn option_parsing() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value".to_string();
        expected.insert("key".to_string(), Some("value".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }

    #[test]
    fn option_parsing_multiple() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value,key2,key3=value7".to_string();
        expected.insert("key".to_string(), Some("value".to_string()));
        expected.insert("key2".to_string(), None);
        expected.insert("key3".to_string(), Some("value7".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }

    #[test]
    fn option_parsing_handle_empty() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value,,,key2,,key3=value7".to_string();
        expected.insert("key".to_string(), Some("value".to_string()));
        expected.insert("key2".to_string(), None);
        expected.insert("key3".to_string(), Some("value7".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }

    #[test]
    fn option_parsing_eq() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value=3,key2,key3=value7=a=b=c".to_string();
        expected.insert("key".to_string(), Some("value=3".to_string()));
        expected.insert("key2".to_string(), None);
        expected.insert("key3".to_string(), Some("value7=a=b=c".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }
}
