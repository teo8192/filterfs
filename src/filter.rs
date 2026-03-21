use log::trace;
use std::path::Path;

use crate::pattern::PatternRule;

pub struct Filter {
    dir_incl: Vec<PatternRule>,
    dir_excl: Vec<PatternRule>,
    file_incl: Vec<PatternRule>,
    file_excl: Vec<PatternRule>,
}

impl Filter {
    pub fn new(file_rules: &[PatternRule], dir_rules: &[PatternRule]) -> Self {
        let mut file_incl = Vec::new();
        let mut file_excl = Vec::new();
        let mut dir_incl = Vec::new();
        let mut dir_excl = Vec::new();

        use PatternRule::*;

        for pattern in file_rules {
            match pattern {
                Include(p) => file_incl.push(Include(p.clone())),
                Exclude(p) => file_excl.push(Exclude(p.clone())),
            }
        }

        for pattern in dir_rules {
            match pattern {
                Include(p) => dir_incl.push(Include(p.clone())),
                Exclude(p) => dir_excl.push(Exclude(p.clone())),
            }
        }

        Self {
            file_incl,
            file_excl,
            dir_incl,
            dir_excl,
        }
    }

    pub fn empty() -> Self {
        Self {
            file_incl: Vec::new(),
            file_excl: Vec::new(),
            dir_incl: Vec::new(),
            dir_excl: Vec::new(),
        }
    }

    pub fn include_file(&self, file: &Path) -> bool {
        let mut include = self.file_incl.is_empty();
        for rule in &self.file_incl {
            if rule.include(file) {
                trace!("Include rule: {:?} matches {:?}!", rule, file);
                include = true;
                break;
            }
        }

        if !include {
            trace!("No include rule matches {:?}!", file);
            return false;
        }

        self.file_excl.iter().all(|rule| rule.include(file))
    }

    pub fn include_dir(&self, dir: &Path) -> bool {
        let mut include = self.dir_incl.is_empty();
        for rule in &self.dir_incl {
            if rule.include(dir) {
                trace!("Include rule: {:?} matches dir {:?}!", rule, dir);
                include = true;
                break;
            }
        }

        if !include {
            trace!("No include rule matches dir {:?}!", dir);
            return false;
        }

        self.dir_excl.iter().all(|rule| rule.include(dir))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{filter::Filter, pattern::PatternRule};

    #[test]
    fn include_simple() {
        let filter = Filter::new(&[PatternRule::new_include("*.txt").unwrap()], &[]);

        // include all dirs, no rules
        assert!(filter.include_dir(&PathBuf::from("")));
        assert!(filter.include_dir(&PathBuf::from("test")));
        assert!(filter.include_dir(&PathBuf::from("test/testd")));

        // matches include rule
        assert!(filter.include_file(&PathBuf::from("test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.txt")));

        // does not match include rule
        assert!(!filter.include_file(&PathBuf::from("test.pdf")));
        assert!(!filter.include_file(&PathBuf::from("dir/test.pdf")));
        assert!(!filter.include_file(&PathBuf::from("dir/subdir/test.pdf")));
    }

    #[test]
    fn exclude_simple() {
        let filter = Filter::new(&[PatternRule::new_exclude("*.pdf").unwrap()], &[]);

        // include all dirs, no rules
        assert!(filter.include_dir(&PathBuf::from("")));
        assert!(filter.include_dir(&PathBuf::from("test")));
        assert!(filter.include_dir(&PathBuf::from("test/testd")));

        // does not match exclude
        assert!(filter.include_file(&PathBuf::from("test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.txt")));

        // matches exclude
        assert!(!filter.include_file(&PathBuf::from("test.pdf")));
        assert!(!filter.include_file(&PathBuf::from("dir/test.pdf")));
        assert!(!filter.include_file(&PathBuf::from("dir/subdir/test.pdf")));
    }

    #[test]
    fn dir_include() {
        let filter = Filter::new(&[], &[PatternRule::new_include("*cool*").unwrap()]);

        // does not match include
        assert!(!filter.include_dir(&PathBuf::from("")));
        assert!(!filter.include_dir(&PathBuf::from("thislamedir")));
        assert!(!filter.include_dir(&PathBuf::from("thislamedir/thislamesubdir")));

        // matches include
        assert!(filter.include_dir(&PathBuf::from("thiscooldir")));
        assert!(filter.include_dir(&PathBuf::from("thiscooldir/thiscoolsubdir")));

        // no rules, include all files
        assert!(filter.include_file(&PathBuf::from("test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.txt")));

        assert!(filter.include_file(&PathBuf::from("test.pdf")));
        assert!(filter.include_file(&PathBuf::from("dir/test.pdf")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.pdf")));
    }

    #[test]
    fn dir_exclude() {
        let filter = Filter::new(&[], &[PatternRule::new_exclude("*lame*").unwrap()]);

        // include does not match
        assert!(!filter.include_dir(&PathBuf::from("")));
        assert!(!filter.include_dir(&PathBuf::from("thislamedir")));
        assert!(!filter.include_dir(&PathBuf::from("thislamedir/thislamesubdir")));

        // include matches
        assert!(filter.include_dir(&PathBuf::from("thiscooldir")));
        assert!(filter.include_dir(&PathBuf::from("thiscooldir/thiscoolsubdir")));

        // all include rules matches for the following files
        assert!(filter.include_file(&PathBuf::from("test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.txt")));

        assert!(filter.include_file(&PathBuf::from("test.pdf")));
        assert!(filter.include_file(&PathBuf::from("dir/test.pdf")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.pdf")));
    }

    #[test]
    fn combined_rules() {
        let filter = Filter::new(
            &[
                PatternRule::new_include("*.txt").unwrap(),
                PatternRule::new_exclude("*hidden*").unwrap(),
            ],
            &[
                PatternRule::new_include("*cool*").unwrap(),
                PatternRule::new_exclude("*lame*").unwrap(),
            ],
        );

        // will not match include rule
        assert!(!filter.include_dir(&PathBuf::from("")));

        // matches exclude rule
        assert!(!filter.include_dir(&PathBuf::from("thislamedir")));
        assert!(!filter.include_dir(&PathBuf::from("thislamedir/thislamesubdir")));

        // matches include but not exclude
        assert!(filter.include_dir(&PathBuf::from("thiscooldir")));
        assert!(filter.include_dir(&PathBuf::from("thiscooldir/thiscoolsubdir")));

        // matches include but also exclude
        assert!(!filter.include_dir(&PathBuf::from("thiscoolbutlamedir")));
        assert!(!filter.include_dir(&PathBuf::from("thiscoolbutlamedir/thiscoolbutlamesubdir")));

        // matches include
        assert!(filter.include_file(&PathBuf::from("test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/test.txt")));
        assert!(filter.include_file(&PathBuf::from("dir/subdir/test.txt")));

        // matches include but also exclude
        assert!(!filter.include_file(&PathBuf::from("testhidden.txt")));
        assert!(!filter.include_file(&PathBuf::from("dir/testhidden.txt")));
        assert!(!filter.include_file(&PathBuf::from("dir/subdir/testhidden.txt")));
    }
}
