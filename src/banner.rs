//! GRID mark as ASCII — diamond outline + solid center square (matches logo.svg).

/// Compact mark (6 lines) for status / help.
pub const MARK: &str = r#"
      /\
     /  \
    / ## \
    \    /
     \  /
      \/
"#;

/// Full banner with wordmark.
pub const BANNER: &str = r#"
      /\
     /  \
    / ## \     G R I D
    \    /     useful mining
     \  /      bitcoin · TSL
      \/
"#;

pub fn print_mark() {
    print!("{}", MARK.trim_start_matches('\n'));
}

pub fn print_banner() {
    print!("{}", BANNER.trim_start_matches('\n'));
}
