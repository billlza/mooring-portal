//! The CFM release the download page offers.
//!
//! The portal does not choose the release: it follows the one GitHub marks
//! as the latest of CFM's repository. The portal's host fetches GitHub's
//! description of that release and hands it to `import-release`;
//! [`CfmRelease::from_github`] reads it.
//!
//! The description comes from the network, so nothing in it is trusted to
//! be what it should be. Three things are kept from it, each checked: the
//! version, the time of publication and the disk image's SHA-256. Nothing
//! else of it reaches the page: every address the page links to is built
//! from the version and [`REPOSITORY`], which is fixed here, so no
//! description, however wrong, can make the page link anywhere else. A
//! description that is a draft or a pre-release, or whose page, disk image
//! or digest is not what GitHub writes for a release of that repository,
//! is refused whole. Of a release's files only the disk image's entry is
//! read; what the entries of the others hold, or whether they are entries
//! at all, is not looked at.
//!
//! No refusal carries text from the description: a refusal is printed to
//! the operator's terminal, or to a service's log when imports are
//! scheduled, and the description's text is a stranger's.

use std::fmt;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::json::Malformation;
use crate::time::UnixTime;

/// CFM's repository. Releases are read from it and from nowhere else.
pub const REPOSITORY: &str = "https://github.com/billlza/cfw-rs";
/// The largest release description accepted, in bytes. GitHub's is about
/// 30 KB for a release of fifteen files; its notes may run to 125,000
/// characters.
pub const MAX_RELEASE_BYTES: usize = 1 << 20;

/// Why a release description was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseError {
    /// Larger than [`MAX_RELEASE_BYTES`].
    #[error("the description is larger than the {MAX_RELEASE_BYTES} bytes a release's may be")]
    TooLarge,
    /// Not JSON, or not a release's description.
    #[error("the description is not a release as GitHub describes one: {0}")]
    Malformed(Malformation),
    /// A draft: not published.
    #[error("the release is a draft")]
    Draft,
    /// A pre-release: published, but not for everyone.
    #[error("the release is a pre-release")]
    Prerelease,
    /// The tag is not `v` and three numbers, alone or with the build
    /// number of a rebuild.
    #[error(
        "the release's tag is not v<major>.<minor>.<patch>, alone or followed by -build.<number>"
    )]
    Tag,
    /// No time of publication, or one not written as GitHub writes it.
    #[error("the release has no time of publication in UTC, to the second")]
    PublishedAt,
    /// The page is not this release's page in the repository.
    #[error("the release's page is not its page in {REPOSITORY}")]
    Page,
    /// No disk image named for the version.
    #[error("the release has no disk image named for its version")]
    NoDiskImage,
    /// Two files with the disk image's name.
    #[error("the release has more than one disk image named for its version")]
    DiskImages,
    /// The disk image's upload is not complete.
    #[error("the release's disk image is not fully uploaded")]
    NotUploaded,
    /// The disk image is empty.
    #[error("the release's disk image is empty")]
    Empty,
    /// The disk image has no SHA-256 as GitHub writes one.
    #[error("the release's disk image has no SHA-256 of 64 lowercase hexadecimal digits")]
    Digest,
    /// The disk image is not downloaded from this release in the repository.
    #[error("the release's disk image is not downloaded from the release in {REPOSITORY}")]
    Download,
}

/// A release's version: three numbers, and a build number when CFM
/// releases a rebuild of them (tagged `v0.5.1-build.51006`). A rebuild
/// comes after the release it rebuilds, and a higher build number after a
/// lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
    /// Compared last, and `None` before any number: the release that was
    /// rebuilt comes before its rebuilds.
    build: Option<u32>,
}

/// What comes between a rebuild's three numbers and its build number.
const BUILD: &str = "-build.";

impl Version {
    /// Parses `major.minor.patch`, or `major.minor.patch-build.number` for
    /// a rebuild: decimal numbers without signs, spaces or leading zeros,
    /// each below 2^32.
    pub fn parse(text: &str) -> Result<Self, ReleaseError> {
        let (numbers, build) = match text.split_once(BUILD) {
            Some((numbers, build)) => (numbers, Some(number(build).ok_or(ReleaseError::Tag)?)),
            None => (text, None),
        };
        let mut parts = numbers.split('.').map(number);
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ReleaseError::Tag);
        };
        Ok(Self {
            major: major.ok_or(ReleaseError::Tag)?,
            minor: minor.ok_or(ReleaseError::Tag)?,
            patch: patch.ok_or(ReleaseError::Tag)?,
            build,
        })
    }

    /// The build number, if the release is a rebuild.
    #[must_use]
    pub const fn build(self) -> Option<u32> {
        self.build
    }

    /// The version a rebuild rebuilds: its three numbers alone.
    #[must_use]
    pub const fn without_build(self) -> Self {
        Self {
            build: None,
            ..self
        }
    }
}

/// A decimal number as a version writes one.
fn number(text: &str) -> Option<u32> {
    let plain = !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    plain.then(|| text.parse().ok()).flatten()
}

impl fmt::Display for Version {
    /// As the tag writes it, after its `v`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(build) = self.build {
            write!(f, "{BUILD}{build}")?;
        }
        Ok(())
    }
}

/// A published macOS release of CFM: what the download page offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfmRelease {
    version: Version,
    published_at: UnixTime,
    dmg_sha256: String,
}

/// A release as GitHub describes it; the keys the portal reads. Its files
/// are kept as they are written until one is known to be the disk image.
#[derive(Deserialize)]
struct Described<'a> {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    #[serde(borrow)]
    assets: Vec<&'a RawValue>,
}

/// What tells one of a release's files from another.
#[derive(Deserialize)]
struct Named {
    name: String,
}

/// The disk image's entry among a release's files.
#[derive(Deserialize)]
struct DiskImage {
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

impl CfmRelease {
    /// Reads GitHub's description of a release (the JSON its REST API
    /// answers for `releases/latest`).
    pub fn from_github(json: &[u8]) -> Result<Self, ReleaseError> {
        if json.len() > MAX_RELEASE_BYTES {
            return Err(ReleaseError::TooLarge);
        }
        let described: Described = serde_json::from_slice(json)
            .map_err(|error| ReleaseError::Malformed(Malformation::of(&error)))?;
        if described.draft {
            return Err(ReleaseError::Draft);
        }
        if described.prerelease {
            return Err(ReleaseError::Prerelease);
        }
        let version = described
            .tag_name
            .strip_prefix('v')
            .ok_or(ReleaseError::Tag)
            .and_then(Version::parse)?;
        let published_at = described
            .published_at
            .as_deref()
            .and_then(|text| UnixTime::parse_utc(text).ok())
            .ok_or(ReleaseError::PublishedAt)?;
        let name = dmg_name(version);
        // An entry that does not say what it is called, once and in text,
        // is not the disk image, whatever else it is.
        let mut images = described.assets.iter().filter(|file| {
            serde_json::from_str::<Named>(file.get()).is_ok_and(|named| named.name == name)
        });
        let image = images.next().ok_or(ReleaseError::NoDiskImage)?;
        if images.next().is_some() {
            return Err(ReleaseError::DiskImages);
        }
        let image: DiskImage = serde_json::from_str(image.get())
            .map_err(|error| ReleaseError::Malformed(Malformation::of(&error)))?;
        if image.state != "uploaded" {
            return Err(ReleaseError::NotUploaded);
        }
        if image.size == 0 {
            return Err(ReleaseError::Empty);
        }
        let digest = image
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or(ReleaseError::Digest)?;
        let release = Self::from_parts(version, published_at, digest)?;
        // What GitHub says the addresses are must be what they are built
        // as here: a description of some other repository's release, or of
        // another tag, is not one to follow.
        if described.html_url != release.page_url() {
            return Err(ReleaseError::Page);
        }
        if image.browser_download_url != release.dmg_url() {
            return Err(ReleaseError::Download);
        }
        Ok(release)
    }

    /// A release from its three kept parts, as the store holds them.
    pub fn from_parts(
        version: Version,
        published_at: UnixTime,
        dmg_sha256: &str,
    ) -> Result<Self, ReleaseError> {
        let hexadecimal = dmg_sha256.len() == 64
            && dmg_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !hexadecimal {
            return Err(ReleaseError::Digest);
        }
        Ok(Self {
            version,
            published_at,
            dmg_sha256: dmg_sha256.to_owned(),
        })
    }

    /// The version.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// When it was published.
    #[must_use]
    pub const fn published_at(&self) -> UnixTime {
        self.published_at
    }

    /// The disk image's SHA-256, as GitHub computed it from the upload: 64
    /// lowercase hexadecimal digits.
    #[must_use]
    pub fn dmg_sha256(&self) -> &str {
        &self.dmg_sha256
    }

    /// The disk image's file name.
    #[must_use]
    pub fn dmg_name(&self) -> String {
        dmg_name(self.version)
    }

    /// Where the disk image is downloaded from.
    #[must_use]
    pub fn dmg_url(&self) -> String {
        format!(
            "{REPOSITORY}/releases/download/v{}/{}",
            self.version,
            self.dmg_name()
        )
    }

    /// The release's page.
    #[must_use]
    pub fn page_url(&self) -> String {
        format!("{REPOSITORY}/releases/tag/v{}", self.version)
    }
}

/// The name CFM's releases give the disk image for Apple silicon. A
/// rebuild's disk image keeps the name of the version it rebuilds; only
/// its tag, and so its address, tells it apart.
fn dmg_name(version: Version) -> String {
    format!("Clash.for.Mac_{}_arm64.dmg", version.without_build())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f";

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_are_three_plain_numbers() {
        for (text, parts) in [
            ("0.5.1", (0, 5, 1)),
            ("0.0.0", (0, 0, 0)),
            ("10.20.30", (10, 20, 30)),
            ("4294967295.0.4294967295", (u32::MAX, 0, u32::MAX)),
        ] {
            let read = version(text);
            assert_eq!((read.major, read.minor, read.patch), parts);
            assert_eq!(read.to_string(), text);
        }
        for bad in [
            "",
            "0",
            "0.5",
            "0.5.1.0",
            "0.5.",
            ".5.1",
            "0..1",
            "0.05.1",
            "00.5.1",
            "0.5.01",
            "0.5.1-rc.1",
            "0.5.1+build",
            "v0.5.1",
            "0.5.x",
            "0.5.-1",
            "0.5.+1",
            " 0.5.1",
            "0.5.1 ",
            "0.5.4294967296",
            "0.5.１",
        ] {
            assert_eq!(Version::parse(bad), Err(ReleaseError::Tag), "{bad:?}");
        }
    }

    #[test]
    fn a_rebuild_adds_its_build_number_to_the_version() {
        for (text, numbers, build) in [
            ("0.5.1-build.51006", (0, 5, 1), 51_006),
            ("0.5.1-build.0", (0, 5, 1), 0),
            ("1.2.3-build.4294967295", (1, 2, 3), u32::MAX),
        ] {
            let read = version(text);
            assert_eq!((read.major, read.minor, read.patch), numbers);
            assert_eq!(read.build(), Some(build));
            assert_eq!(read.to_string(), text);
            let rebuilt = read.without_build();
            assert_eq!(rebuilt.build(), None);
            assert_eq!(rebuilt.to_string(), text.split_once('-').unwrap().0);
        }
        assert_eq!(version("0.5.1").build(), None);
        assert_eq!(version("0.5.1").without_build(), version("0.5.1"));
        for bad in [
            "0.5.1-build",
            "0.5.1-build.",
            "0.5.1-build.051006",
            "0.5.1-build.x",
            "0.5.1-build.1.2",
            "0.5.1-build.1-build.2",
            "0.5.1-Build.1",
            "0.5.1-build1",
            "0.5.1.build.1",
            "0.5.1--build.1",
            "0.5.1-build.-1",
            "0.5.1-build.+1",
            "0.5.1-build. 1",
            "0.5.1-build.1 ",
            "0.5.1-build.4294967296",
            "0.5.1-build.１",
            "0.5-build.1",
            "-build.1",
            "0.5.1+build.1",
            "0.5.1-rc.1-build.1",
        ] {
            assert_eq!(Version::parse(bad), Err(ReleaseError::Tag), "{bad:?}");
        }
    }

    #[test]
    fn a_rebuild_is_downloaded_from_its_own_tag_under_its_versions_name() {
        let published = UnixTime::from_secs(1_794_513_803).unwrap();
        let release = CfmRelease::from_parts(version("0.5.1-build.51006"), published, SHA).unwrap();
        assert_eq!(release.version(), version("0.5.1-build.51006"));
        assert_eq!(release.dmg_name(), "Clash.for.Mac_0.5.1_arm64.dmg");
        assert_eq!(
            release.dmg_url(),
            format!(
                "{REPOSITORY}/releases/download/v0.5.1-build.51006/Clash.for.Mac_0.5.1_arm64.dmg"
            )
        );
        assert_eq!(
            release.page_url(),
            format!("{REPOSITORY}/releases/tag/v0.5.1-build.51006")
        );
    }

    #[test]
    fn versions_order_by_number_not_by_text() {
        let ordered = [
            "0.5.1",
            "0.5.1-build.0",
            "0.5.1-build.9",
            "0.5.1-build.10",
            "0.5.1-build.51006",
            "0.5.9",
            "0.5.10",
            "0.6.0",
            "0.10.0",
            "1.0.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(version(pair[0]) < version(pair[1]), "{pair:?}");
        }
        assert_eq!(version("0.5.1"), version("0.5.1"));
    }

    #[test]
    fn a_release_is_rebuilt_from_its_parts_and_its_digest_checked() {
        let published = UnixTime::from_secs(1_794_065_221).unwrap();
        let release = CfmRelease::from_parts(version("0.5.1"), published, SHA).unwrap();
        assert_eq!(release.version(), version("0.5.1"));
        assert_eq!(release.published_at(), published);
        assert_eq!(release.dmg_sha256(), SHA);
        assert_eq!(release.dmg_name(), "Clash.for.Mac_0.5.1_arm64.dmg");
        assert_eq!(
            release.dmg_url(),
            format!("{REPOSITORY}/releases/download/v0.5.1/Clash.for.Mac_0.5.1_arm64.dmg")
        );
        assert_eq!(
            release.page_url(),
            format!("{REPOSITORY}/releases/tag/v0.5.1")
        );
        for bad in [
            "",
            &SHA[1..],
            &format!("{SHA}0"),
            &SHA.to_uppercase(),
            &SHA.replace('c', "g"),
            &format!("sha256:{}", &SHA[7..]),
        ] {
            assert_eq!(
                CfmRelease::from_parts(version("0.5.1"), published, bad),
                Err(ReleaseError::Digest),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn refusals_name_the_repository_and_nothing_of_the_description() {
        assert_eq!(
            ReleaseError::Page.to_string(),
            "the release's page is not its page in https://github.com/billlza/cfw-rs"
        );
        assert_eq!(
            ReleaseError::Malformed(Malformation::Truncated).to_string(),
            "the description is not a release as GitHub describes one: it ends too early"
        );
        assert_eq!(
            ReleaseError::TooLarge.to_string(),
            "the description is larger than the 1048576 bytes a release's may be"
        );
    }
}
