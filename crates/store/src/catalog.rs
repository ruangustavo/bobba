use std::collections::HashMap;

use serde::Deserialize;

use crate::runtime::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    fn store_name(self) -> &'static str {
        match self {
            Os::Mac => "Mac",
            Os::Windows => "Windows",
            Os::Linux => "Linux",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub title: String,
    pub description: String,
    pub authors: Vec<String>,
    pub version: String,
    pub categories: Vec<String>,
    pub framework: String,
    pub rating: u32,
    pub updated: String,
    pub source: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framework {
    pub name: String,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub listings: Vec<Listing>,
    pub categories: Vec<Category>,
    pub frameworks: Vec<Framework>,
}

impl Listing {
    pub fn runtimes(&self) -> Vec<Runtime> {
        let mut runtimes = Vec::new();
        let found = self
            .command
            .first()
            .and_then(|program| Runtime::of_program(program));
        for runtime in found
            .into_iter()
            .chain(Runtime::of_framework(&self.framework))
        {
            if !runtimes.contains(&runtime) {
                runtimes.push(runtime);
            }
        }
        runtimes
    }
}

impl Catalog {
    pub fn parse(config: &str, extensions: &str, os: Os) -> Result<Self, serde_json::Error> {
        let config: RawConfig = serde_json::from_str(config)?;
        let extensions: Vec<RawExtension> = serde_json::from_str(extensions)?;
        let mut listings: Vec<Listing> = extensions
            .into_iter()
            .filter_map(|raw| raw.into_listing(os))
            .collect();
        listings.sort_by(|a, b| b.rating.cmp(&a.rating).then(a.title.cmp(&b.title)));
        Ok(Catalog {
            listings,
            categories: config
                .categories
                .into_iter()
                .map(|c| Category {
                    name: c.name,
                    description: c.description,
                })
                .collect(),
            frameworks: config
                .frameworks
                .into_iter()
                .map(|f| Framework {
                    name: f.name,
                    instructions: f
                        .installation
                        .filter(|i| i.required)
                        .and_then(|i| i.instructions),
                })
                .collect(),
        })
    }

    pub fn listing(&self, title: &str) -> Option<&Listing> {
        self.listings.iter().find(|l| l.title == title)
    }

    pub fn framework(&self, name: &str) -> Option<&Framework> {
        self.frameworks.iter().find(|f| f.name == name)
    }
}

pub fn is_newer(candidate: &str, installed: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(candidate), parts(installed));
    let len = a.len().max(b.len());
    let at = |v: &Vec<u64>, i: usize| v.get(i).copied().unwrap_or(0);
    (0..len)
        .map(|i| at(&a, i).cmp(&at(&b, i)))
        .find(|o| o.is_ne())
        .is_some_and(|o| o.is_gt())
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default)]
    categories: Vec<RawCategory>,
    #[serde(default)]
    frameworks: Vec<RawFramework>,
}

#[derive(Deserialize)]
struct RawCategory {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct RawFramework {
    name: String,
    installation: Option<RawInstallation>,
}

#[derive(Deserialize)]
struct RawInstallation {
    #[serde(default)]
    required: bool,
    instructions: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExtension {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    authors: Vec<RawAuthor>,
    version: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    source: String,
    framework: RawFrameworkRef,
    commands: HashMap<String, Option<Vec<String>>>,
    compatibility: RawCompatibility,
    #[serde(default)]
    update_date: String,
    #[serde(default)]
    is_outdated: bool,
    #[serde(default)]
    rating: Option<u32>,
}

#[derive(Deserialize)]
struct RawAuthor {
    name: String,
}

#[derive(Deserialize)]
struct RawFrameworkRef {
    name: String,
}

#[derive(Deserialize)]
struct RawCompatibility {
    #[serde(default)]
    systems: Vec<String>,
    #[serde(default)]
    clients: Vec<String>,
}

impl RawExtension {
    fn into_listing(self, os: Os) -> Option<Listing> {
        let runs_here = self
            .compatibility
            .systems
            .iter()
            .any(|s| s == os.store_name());
        let flash = self.compatibility.clients.iter().any(|c| c == "Flash");
        let plain_title = !self.title.is_empty() && !self.title.contains(['/', '\\']);
        if self.is_outdated || !flash || !runs_here || !plain_title {
            return None;
        }
        let key = match os {
            Os::Mac => "mac",
            Os::Windows => "windows",
            Os::Linux => "linux",
        };
        let mut commands = self.commands;
        let command = commands
            .remove(key)
            .flatten()
            .filter(|c| !c.is_empty())
            .or_else(|| commands.remove("default").flatten())
            .filter(|c| !c.is_empty())?;
        Some(Listing {
            title: self.title,
            description: self.description,
            authors: self.authors.into_iter().map(|a| a.name).collect(),
            version: self.version,
            categories: self.categories,
            framework: self.framework.name,
            rating: self.rating.unwrap_or(0),
            updated: self.update_date,
            source: self.source,
            command,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
        "categories": [{"name": "Building", "description": "Tools that improve building", "icon": "b.png"}],
        "frameworks": [
            {"name": "Native", "installation": {"required": false, "instructions": null}},
            {"name": "G-Node", "installation": {"required": true, "instructions": "https://example.test/node"}}
        ]
    }"#;

    fn extension(title: &str, extra: &str) -> String {
        format!(
            r#"{{"title": "{title}", "description": "d", "authors": [{{"name": "someone"}}],
                "version": "1.2.0", "categories": ["Building"], "source": "https://example.test",
                "framework": {{"name": "Native", "version": "1.5"}},
                "commands": {{"default": ["java", "-jar", "a.jar"]}},
                "compatibility": {{"systems": ["Windows", "Mac", "Linux"], "clients": ["Flash"]}},
                "updateDate": "01-11-2026 10:00:00", "isOutdated": false, "rating": 10 {extra}}}"#
        )
    }

    fn parse(items: &[String], os: Os) -> Catalog {
        Catalog::parse(CONFIG, &format!("[{}]", items.join(",")), os).expect("parses")
    }

    #[test]
    fn keeps_flash_extensions_that_run_here_and_are_current() {
        let origins = extension("Origins Only", "")
            .replace(r#""clients": ["Flash"]"#, r#""clients": ["Origins"]"#);
        let outdated =
            extension("Old", "").replace(r#""isOutdated": false"#, r#""isOutdated": true"#);
        let windows_only = extension("Win", "").replace(
            r#""systems": ["Windows", "Mac", "Linux"]"#,
            r#""systems": ["Windows"]"#,
        );
        let items = [extension("Keep", ""), origins, outdated, windows_only];

        let on_mac = parse(&items, Os::Mac);
        let on_windows = parse(&items, Os::Windows);

        let titles = |c: &Catalog| {
            c.listings
                .iter()
                .map(|l| l.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(&on_mac), ["Keep"]);
        assert_eq!(titles(&on_windows), ["Keep", "Win"]);
        assert_eq!(on_mac.listings[0].authors, ["someone"]);
    }

    #[test]
    fn picks_the_command_for_this_system_and_falls_back_to_default() {
        let item = extension("Both", "").replace(
            r#""commands": {"default": ["java", "-jar", "a.jar"]}"#,
            r#""commands": {"default": ["cmd", "/C", "a.exe"], "mac": ["/bin/sh", "-c", "./a"], "linux": null}"#,
        );
        assert_eq!(
            parse(std::slice::from_ref(&item), Os::Mac).listings[0].command,
            ["/bin/sh", "-c", "./a"]
        );
        assert_eq!(
            parse(std::slice::from_ref(&item), Os::Linux).listings[0].command,
            ["cmd", "/C", "a.exe"]
        );
        assert_eq!(
            parse(&[item], Os::Windows).listings[0].command,
            ["cmd", "/C", "a.exe"]
        );
    }

    #[test]
    fn orders_by_rating_and_reads_framework_instructions() {
        let low = extension("Low", "").replace(r#""rating": 10"#, r#""rating": 3"#);
        let high = extension("High", "").replace(r#""rating": 10"#, r#""rating": 90"#);
        let catalog = parse(&[low, high], Os::Mac);
        assert_eq!(catalog.listings[0].title, "High");
        assert_eq!(
            catalog
                .framework("Native")
                .and_then(|f| f.instructions.clone()),
            None
        );
        assert_eq!(
            catalog
                .framework("G-Node")
                .and_then(|f| f.instructions.clone())
                .as_deref(),
            Some("https://example.test/node")
        );
        assert_eq!(catalog.categories[0].name, "Building");
    }

    fn listing_with(framework: &str, command: &str) -> Listing {
        let item = extension("Any", "")
            .replace(
                r#""name": "Native", "version""#,
                &format!(r#""name": "{framework}", "version""#),
            )
            .replace(r#"["java", "-jar", "a.jar"]"#, command);
        parse(&[item], Os::Windows).listings.remove(0)
    }

    #[test]
    fn a_listing_needs_the_runtime_of_its_command_and_of_its_framework() {
        let needs = |framework, command| listing_with(framework, command).runtimes();
        assert_eq!(
            needs("Native", r#"["java", "-jar", "a.jar"]"#),
            [Runtime::Java]
        );
        assert_eq!(
            needs(
                "G-Node",
                r#"["java", "-jar", "NodeExtensionSetup.jar", "-npm", "start"]"#
            ),
            [Runtime::Java, Runtime::NodeJs]
        );
        assert_eq!(
            needs("G-Node", r#"["node", "index.js"]"#),
            [Runtime::NodeJs]
        );
        assert_eq!(
            needs("G-Python", r#"["python", "main.py"]"#),
            [Runtime::Python]
        );
        assert_eq!(needs("QX", r#"["cmd", "/C", "QX.exe"]"#), [Runtime::DotNet]);
        assert_eq!(
            needs("Geode", r#"["cmd", "/C", "LTDHelper.exe"]"#),
            [Runtime::DotNet]
        );
        assert_eq!(needs("Xabbo", r#"["cmd", "/C", "xabbo.exe"]"#), []);
        assert_eq!(needs("GoEarth", r#"["/bin/sh", "-c", "./a"]"#), []);
    }

    #[test]
    fn skips_titles_that_cannot_be_a_folder_name() {
        let slash = extension("a/b", "");
        let backslash = extension(r"a\\b", "");
        assert!(parse(&[slash, backslash], Os::Mac).listings.is_empty());
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(is_newer("2.0", "1.99"));
        assert!(is_newer("1.0.1", "1.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("0.9", "1.0"));
    }
}
