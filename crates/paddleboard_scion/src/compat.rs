use semver::Version;

pub const TESTED_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    Compatible,
    NewerThanTested { installed: String, tested: String },
    OlderThanTested { installed: String, tested: String },
    Unparseable { raw: String },
}

pub fn check_compatibility(version_string: &str) -> Compatibility {
    let trimmed = version_string.trim().trim_start_matches('v');
    if trimmed.is_empty() {
        return Compatibility::Unparseable {
            raw: version_string.to_string(),
        };
    }

    let installed = match Version::parse(trimmed) {
        Ok(v) => v,
        Err(_) => {
            return Compatibility::Unparseable {
                raw: version_string.to_string(),
            };
        }
    };

    let tested = match Version::parse(TESTED_VERSION) {
        Ok(v) => v,
        Err(_) => {
            return Compatibility::Unparseable {
                raw: format!("bad TESTED_VERSION: {TESTED_VERSION}"),
            };
        }
    };

    if installed.major == tested.major && installed.minor == tested.minor {
        Compatibility::Compatible
    } else if installed > tested {
        Compatibility::NewerThanTested {
            installed: installed.to_string(),
            tested: tested.to_string(),
        }
    } else {
        Compatibility::OlderThanTested {
            installed: installed.to_string(),
            tested: tested.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_is_compatible() {
        assert_eq!(
            check_compatibility("0.1.0"),
            Compatibility::Compatible
        );
    }

    #[test]
    fn patch_version_is_compatible() {
        assert_eq!(
            check_compatibility("0.1.5"),
            Compatibility::Compatible
        );
    }

    #[test]
    fn v_prefix_stripped() {
        assert_eq!(
            check_compatibility("v0.1.0"),
            Compatibility::Compatible
        );
    }

    #[test]
    fn newer_minor_is_newer() {
        assert_eq!(
            check_compatibility("0.2.0"),
            Compatibility::NewerThanTested {
                installed: "0.2.0".into(),
                tested: "0.1.0".into(),
            }
        );
    }

    #[test]
    fn newer_major_is_newer() {
        assert_eq!(
            check_compatibility("1.0.0"),
            Compatibility::NewerThanTested {
                installed: "1.0.0".into(),
                tested: "0.1.0".into(),
            }
        );
    }

    #[test]
    fn older_is_older() {
        assert_eq!(
            check_compatibility("0.0.9"),
            Compatibility::OlderThanTested {
                installed: "0.0.9".into(),
                tested: "0.1.0".into(),
            }
        );
    }

    #[test]
    fn empty_string_is_unparseable() {
        assert!(matches!(
            check_compatibility(""),
            Compatibility::Unparseable { .. }
        ));
    }

    #[test]
    fn unknown_is_unparseable() {
        assert!(matches!(
            check_compatibility("unknown"),
            Compatibility::Unparseable { .. }
        ));
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(
            check_compatibility("  0.1.3  "),
            Compatibility::Compatible
        );
    }
}
