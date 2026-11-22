//! Release versions: three numbers that compare in order.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A release version: major, minor and patch, compared in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

/// Text that isn't three dot-separated numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAVersion;

impl fmt::Display for NotAVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a version of three numbers")
    }
}

impl std::error::Error for NotAVersion {}

impl FromStr for Version {
    type Err = NotAVersion;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let number = |part: &str| {
            let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            digits
                .then(|| part.parse().ok())
                .flatten()
                .ok_or(NotAVersion)
        };
        let mut parts = text.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(NotAVersion);
        };
        Ok(Self {
            major: number(major)?,
            minor: number(minor)?,
            patch: number(patch)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl TryFrom<String> for Version {
    type Error = NotAVersion;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Version> for String {
    fn from(version: Version) -> Self {
        version.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn versions_compare_by_number_and_reject_other_shapes() {
        let parse = |text: &str| text.parse::<Version>();
        assert!(parse("0.2.10").expect("a version") > parse("0.2.9").expect("a version"));
        assert!(parse("1.0.0").expect("a version") > parse("0.99.99").expect("a version"));
        assert_eq!(parse("0.3.1").expect("a version").to_string(), "0.3.1");
        for bad in ["1.2", "1.2.3.4", "a.b.c", "1.-2.3", ""] {
            assert!(parse(bad).is_err(), "{bad} is not a version");
        }
    }
}
