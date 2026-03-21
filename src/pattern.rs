use glob::{Pattern, PatternError};
use std::path::Path;

#[derive(Debug)]
pub enum PatternRule {
    Include(Pattern),
    Exclude(Pattern),
}

impl PatternRule {
    pub fn new_include(glob: &str) -> Result<Self, PatternError> {
        Pattern::new(glob).map(Self::Include)
    }

    pub fn new_exclude(glob: &str) -> Result<Self, PatternError> {
        Pattern::new(glob).map(Self::Exclude)
    }

    /// Returns true if the filename should be included
    /// Will return false if pattern matching fails in any way, related to string conversions and
    /// so on.
    /// ```
    /// # use filterfs::pattern::PatternRule::*;
    /// # use glob::Pattern;
    /// # use std::path::Path;
    /// # let txt_file = Path::new("/some/file.txt");
    /// # let pdf_file = Path::new("/some/file.pdf");
    /// let include_txt = Include(Pattern::new("*.txt").unwrap());
    ///
    /// assert!(include_txt.include(&txt_file));
    /// assert!(!include_txt.include(&pdf_file));
    ///
    /// let exclude_txt = Exclude(Pattern::new("*.txt").unwrap());
    ///
    /// assert!(!exclude_txt.include(&txt_file));
    /// assert!(exclude_txt.include(&pdf_file));
    /// ```
    pub fn include(&self, file: &Path) -> bool {
        fn try_match(pr: &PatternRule, file: &Path) -> Option<bool> {
            use PatternRule::*;
            if let Some(filename) = file.file_name() {
                Some(match pr {
                    Include(p) => p.matches(filename.to_str()?),
                    Exclude(p) => !p.matches(filename.to_str()?),
                })
            } else {
                None
            }
        }

        try_match(self, file).unwrap_or(false)
    }
}
