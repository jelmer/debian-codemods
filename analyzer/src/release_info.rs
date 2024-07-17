use chrono::{NaiveDate, Utc};
use distro_info::DistroInfo;

pub fn debian_releases() -> Vec<String> {
    let debian = distro_info::DebianDistroInfo::new().unwrap();
    debian
        .all_at(Utc::now().naive_utc().date())
        .iter()
        .map(|r| r.series().to_string())
        .collect()
}

pub fn ubuntu_releases() -> Vec<String> {
    let ubuntu = distro_info::UbuntuDistroInfo::new().unwrap();
    ubuntu
        .all_at(Utc::now().naive_utc().date())
        .iter()
        .map(|r| r.series().to_string())
        .collect()
}

pub fn resolve_release_codename(name: &str, date: Option<NaiveDate>) -> Option<String> {
    let date = date.unwrap_or(Utc::now().naive_utc().date());
    let (distro, mut name) = if let Some((distro, name)) = name.split_once('/') {
        (Some(distro), name)
    } else {
        (None, name)
    };
    let active = |x: &Option<NaiveDate>| x.map(|x| x > date).unwrap_or(false);
    if distro.is_none() || distro == Some("debian") {
        let debian = distro_info::DebianDistroInfo::new().unwrap();
        if name == "lts" {
            let lts = debian
                .all_at(date)
                .into_iter()
                .filter(|r| active(r.eol_lts()))
                .min_by_key(|r| r.created());
            return lts.map(|r| r.series().to_string());
        }
        if name == "elts" {
            let elts = debian
                .all_at(date)
                .into_iter()
                .filter(|r| active(r.eol_elts()))
                .min_by_key(|r| r.created());
            return elts.map(|r| r.series().to_string());
        }
        let mut all_released = debian
            .all_at(date)
            .into_iter()
            .filter(|r| r.release().is_some())
            .collect::<Vec<_>>();
        all_released.sort_by_key(|r| r.created());
        all_released.reverse();
        if name == "stable" {
            return Some(all_released[0].series().to_string());
        }
        if name == "oldstable" {
            return Some(all_released[1].series().to_string());
        }
        if name == "oldoldstable" {
            return Some(all_released[2].series().to_string());
        }
        if name == "unstable" {
            name = "sid";
        }
        if name == "testing" {
            let mut all_unreleased = debian
                .all_at(date)
                .into_iter()
                .filter(|r| r.release().is_none())
                .collect::<Vec<_>>();
            all_unreleased.sort_by_key(|r| r.created());
            return Some(all_unreleased.last().unwrap().series().to_string());
        }

        let all = debian.all_at(date);
        if let Some(series) = all
            .iter()
            .find(|r| r.codename() == name || r.series() == name)
        {
            return Some(series.series().to_string());
        }
    }
    if distro.is_none() || distro == Some("ubuntu") {
        let ubuntu = distro_info::UbuntuDistroInfo::new().unwrap();
        if name == "esm" {
            return ubuntu
                .all_at(date)
                .into_iter()
                .filter(|r| active(r.eol_esm()))
                .min_by_key(|r| r.created())
                .map(|r| r.series().to_string());
        }
        if name == "lts" {
            return ubuntu
                .all_at(date)
                .into_iter()
                .filter(|r| r.is_lts() && r.supported_at(date))
                .min_by_key(|r| r.created())
                .map(|r| r.series().to_string());
        }
        let all = ubuntu.all_at(date);
        if let Some(series) = all
            .iter()
            .find(|r| r.codename() == name || r.series() == name)
        {
            return Some(series.series().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::resolve_release_codename;

    #[test]
    fn test_debian() {
        assert_eq!("sid", resolve_release_codename("debian/sid", None).unwrap());
        assert_eq!("sid", resolve_release_codename("sid", None).unwrap());
        assert_eq!("sid", resolve_release_codename("unstable", None).unwrap());
        assert_eq!(
            "experimental",
            resolve_release_codename("experimental", None).unwrap()
        );
    }

    #[test]
    fn test_ubuntu() {
        assert_eq!(
            "trusty",
            resolve_release_codename("ubuntu/trusty", None).unwrap()
        );
        assert_eq!("trusty", resolve_release_codename("trusty", None).unwrap());
        assert!(resolve_release_codename("ubuntu/lts", None).is_some());
    }

    #[test]
    fn test_resolve_debian() {
        assert_eq!("sid", resolve_release_codename("sid", None).unwrap());
        assert_eq!("buster", resolve_release_codename("buster", None).unwrap());
        assert_eq!("sid", resolve_release_codename("unstable", None).unwrap());
        assert_eq!(
            "sid",
            resolve_release_codename("debian/unstable", None).unwrap()
        );
        assert!(resolve_release_codename("oldstable", None).is_some());
        assert!(resolve_release_codename("oldoldstable", None).is_some());
    }

    #[test]
    fn test_resolve_unknown() {
        assert!(resolve_release_codename("blah", None).is_none());
    }

    #[test]
    fn test_resolve_ubuntu() {
        assert_eq!("trusty", resolve_release_codename("trusty", None).unwrap());
        assert_eq!(
            "trusty",
            resolve_release_codename("ubuntu/trusty", None).unwrap()
        );
        assert!(resolve_release_codename("ubuntu/lts", None).is_some())
    }

    #[test]
    fn test_resolve_ubuntu_esm() {
        assert!(resolve_release_codename("ubuntu/esm", None).is_some())
    }
}
