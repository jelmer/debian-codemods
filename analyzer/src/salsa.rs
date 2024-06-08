use std::collections::HashMap;
use url::Url;

lazy_static::lazy_static! {
static ref MAINTAINER_EMAIL_MAP: HashMap<&'static str, &'static str> = maplit::hashmap! {
    "pkg-javascript-devel@lists.alioth.debian.org" => "js-team",
    "python-modules-team@lists.alioth.debian.org" => "python-team/modules",
    "python-apps-team@lists.alioth.debian.org" => "python-team/applications",
    "debian-science-maintainers@lists.alioth.debian.org" => "science-team",
    "pkg-perl-maintainers@lists.alioth.debian.org" =>
        "perl-team/modules/packages",
    "pkg-java-maintainers@lists.alioth.debian.org" => "java-team",
    "pkg-ruby-extras-maintainers@lists.alioth.debian.org" => "ruby-team",
    "pkg-clamav-devel@lists.alioth.debian.org" => "clamav-team",
    "pkg-go-maintainers@lists.alioth.debian.org" => "go-team/packages",
    "pkg-games-devel@lists.alioth.debian.org" => "games-team",
    "pkg-telepathy-maintainers@lists.alioth.debian.org" => "telepathy-team",
    "debian-fonts@lists.debian.org" => "fonts-team",
    "pkg-gnustep-maintainers@lists.alioth.debian.org" => "gnustep-team",
    "pkg-gnome-maintainers@lists.alioth.debian.org" => "gnome-team",
    "pkg-multimedia-maintainers@lists.alioth.debian.org" => "multimedia-team",
    "debian-ocaml-maint@lists.debian.org" => "ocaml-team",
    "pkg-php-pear@lists.alioth.debian.org" => "php-team/pear",
    "pkg-mpd-maintainers@lists.alioth.debian.org" => "mpd-team",
    "pkg-cli-apps-team@lists.alioth.debian.org" => "dotnet-team",
    "pkg-mono-group@lists.alioth.debian.org" => "dotnet-team",
    "team+python@tracker.debian.org" => "python-team/packages",
};
}

/// Guess the repository URL for a package hosted on Salsa.
///
/// # Arguments:
/// * `package`: Package name
/// * `maintainer_email`: The maintainer's email address (e.g. team list address)
///
/// # Returns:
/// A guessed repository URL
pub fn guess_repository_url(package: &str, maintainer_email: &str) -> Option<Url> {
    let team_name = if maintainer_email.ends_with("@debian.org") {
        maintainer_email.split('@').next().unwrap()
    } else if let Some(team_name) = MAINTAINER_EMAIL_MAP.get(maintainer_email) {
        team_name
    } else {
        return None;
    };

    format!("https://salsa.debian.org/{}/{}.git", team_name, package)
        .parse()
        .ok()
}

#[cfg(test)]
mod guess_repository_url_tests {
    use super::*;

    #[test]
    fn test_unknown() {
        assert_eq!(
            None,
            guess_repository_url("blah", "unknown-team@lists.alioth.debian.org")
        );
    }

    #[test]
    fn test_individual() {
        assert_eq!(
            Some(
                "https://salsa.debian.org/jelmer/lintian-brush.git"
                    .parse()
                    .unwrap()
            ),
            guess_repository_url("lintian-brush", "jelmer@debian.org")
        );
    }

    #[test]
    fn test_team() {
        assert_eq!(
            Some(
                "https://salsa.debian.org/js-team/node-blah.git"
                    .parse()
                    .unwrap()
            ),
            guess_repository_url("node-blah", "pkg-javascript-devel@lists.alioth.debian.org")
        );
    }
}
