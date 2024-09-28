//! Information about the distribution vendor
use deb822_lossless::{Deb822, Paragraph};

fn load_vendor_file(name: Option<&str>) -> std::io::Result<Deb822> {
    let name = name.unwrap_or("default");

    let path = std::path::Path::new("/etc/dpkg/origins").join(name);

    let f = std::fs::read_to_string(path)?;

    Ok(f.parse().unwrap())
}

/// Information about a distribution vendor
pub struct Vendor {
    /// The name of the vendor (e.g. "Debian", "Ubuntu")
    pub name: String,

    /// The URL of the bug tracker (e.g. "https://bugs.debian.org/")
    pub bugs: url::Url,

    /// The homepage of the vendor (e.g. "https://www.debian.org/")
    pub url: url::Url,
}

impl std::str::FromStr for Vendor {
    type Err = deb822_lossless::ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let data = Deb822::from_str(text)?;

        let data = data.paragraphs().next().unwrap();

        Ok(data.into())
    }
}

impl From<Paragraph> for Vendor {
    fn from(data: Paragraph) -> Self {
        // TODO: rely on derive
        Vendor {
            name: data.get("Vendor").unwrap(),
            url: data.get("Vendor-URL").unwrap().parse().unwrap(),
            bugs: data.get("Bugs").unwrap().parse().unwrap(),
        }
    }
}

/// Get the vendor information for a given vendor name
pub fn get_vendor(name: Option<&str>) -> std::io::Result<Vendor> {
    let data = load_vendor_file(name)?;

    Ok(data.paragraphs().next().unwrap().into())
}

/// Get the vendor name for the current system
pub fn get_vendor_name() -> std::io::Result<String> {
    if let Ok(vendor) = std::env::var("DEB_VENDOR") {
        Ok(vendor)
    } else {
        Ok(get_vendor(None)?.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_get_vendor_name() {
        let _ = get_vendor_name();
    }

    #[test]
    fn test_paragraph_to_vendor() {
        let data = r#"Vendor: Debian
Vendor-URL: https://www.debian.org/
Bugs: https://bugs.debian.org/"#;

        let vendor: Vendor = data.parse().unwrap();

        assert_eq!(vendor.name, "Debian");
        assert_eq!(vendor.bugs, "https://bugs.debian.org/".parse().unwrap());
        assert_eq!(vendor.url, "https://www.debian.org/".parse().unwrap());
    }
}
