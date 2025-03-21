use flash_macros::decl_config;
use glob::glob;
use regex_lite::Regex;
use serde::{Deserialize, Deserializer};
use std::{fs, path::PathBuf, sync::Arc};

use crate::url::UrlPath;

fn parse_template<'de, D>(deserializer: D) -> Result<Arc<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Arc::from(
        fs::read_to_string(PathBuf::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)?,
    ))
}

fn parse_sources<'de, D>(deserializer: D) -> Result<Vec<Arc<Source>>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Vec::<RawSource>::deserialize(deserializer)?
        .into_iter()
        .map(|src| Arc::from(Source::from_raw(src).unwrap()))
        .collect())
}

fn parse_glob<'de, D>(deserializer: D) -> Result<Vec<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Vec::<PathBuf>::deserialize(deserializer)?
        .iter()
        .flat_map(|src| {
            glob(src.to_str().unwrap())
                .unwrap_or_else(|_| panic!("Invalid glob pattern {}", src.to_str().unwrap()))
                .map(|g| g.unwrap())
        })
        .collect())
}

pub struct MyRegex(Regex);

impl<'de> serde::Deserialize<'de> for MyRegex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Regex::new(&s)
            .map_err(serde::de::Error::custom)
            .map(MyRegex)
    }
}

impl std::ops::Deref for MyRegex {
    type Target = Regex;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

macro_rules! default_template {
    ($name: expr) => {
        Arc::from(include_str!($name).to_string())
    };
}

macro_rules! default_scripts {
    () => {
        Vec::new(),
    };

    (@ $name: expr) => {
        Script {
            name: $name.into(),
            content: default_template!(concat!("../templates/", $name)),
        }
    };

    ($name: expr $(, $rest: expr)*) => {
        vec![default_scripts!(@ $name), $(default_scripts!(@ $rest)),*]
    };
}

#[derive(Debug)]
pub struct Source {
    pub name: String,
    pub dir: UrlPath,
    pub include: Vec<PathBuf>,
    pub exists_online: bool,
}

impl Source {
    pub fn from_raw(src: RawSource) -> Result<Source, String> {
        let exclude = src
            .exclude
            .into_iter()
            .map(|p| src.dir.to_pathbuf().join(p))
            .flat_map(|src| {
                glob(src.to_str().unwrap())
                    .unwrap_or_else(|_| panic!("Invalid glob pattern {}", src.to_str().unwrap()))
                    .map(|g| g.unwrap())
            })
            .collect::<Vec<_>>();

        let include = src
            .include
            .into_iter()
            .map(|p| src.dir.to_pathbuf().join(p))
            .flat_map(|src| {
                glob(src.to_str().unwrap())
                    .unwrap_or_else(|_| panic!("Invalid glob pattern {}", src.to_str().unwrap()))
                    .map(|g| g.unwrap())
            })
            .filter(|p| !exclude.contains(p))
            .collect::<Vec<_>>();

        Ok(Self {
            name: src.name,
            dir: src.dir,
            exists_online: src.exists_online,
            include,
        })
    }
}

decl_config! {
    struct Script {
        name: String,
        content: Arc<String> as parse_template,
    }

    struct RawSource {
        name: String,
        dir: UrlPath,
        include: Vec<PathBuf>,
        exclude: Vec<PathBuf> = Vec::new(),
        exists_online: bool = true,
    }

    struct ExternalLib {
        pattern: String,
        repository: String,
    }

    struct RegexPattern {
        patterns_full: Vec<MyRegex> = Vec::new(),
        patterns_name: Vec<MyRegex> = Vec::new(),
    }

    struct Config {
        project {
            name: String,
            version: String,
            repository?: String,
            tree?: String,
            icon?: PathBuf,
        },
        tutorials? {
            dir: PathBuf,
            assets: Vec<PathBuf> as parse_glob = Vec::new(),
        },
        sources: Vec<Arc<Source>> as parse_sources,
        run? {
            prebuild: Vec<String> = Vec::new(),
        },
        analysis {
            compile_args: Vec<String> = Vec::new(),
        },
        cmake? {
            config_args: Vec<String> = Vec::new(),
            build_args: Vec<String> = Vec::new(),
            build: bool = false,
            build_dir: String = String::from("build"),
            infer_args_from: PathBuf,
        },
        templates {
            class:          Arc<String> as parse_template = default_template!("../templates/class.html"),
            struct_:        Arc<String> as parse_template = default_template!("../templates/struct.html"),
            function:       Arc<String> as parse_template = default_template!("../templates/function.html"),
            head:           Arc<String> as parse_template = default_template!("../templates/head.html"),
            nav:            Arc<String> as parse_template = default_template!("../templates/nav.html"),
            file:           Arc<String> as parse_template = default_template!("../templates/file.html"),
            page:           Arc<String> as parse_template = default_template!("../templates/page.html"),
            tutorial:       Arc<String> as parse_template = default_template!("../templates/tutorial.html"),
            tutorial_index: Arc<String> as parse_template = default_template!("../templates/tutorial-index.html"),
        },
        scripts {
            css: Vec<Script> = default_scripts!("default.css", "nav.css", "content.css", "themes.css"),
            js:  Vec<Script> = default_scripts!("script.js"),
        },
        external_libs: Vec<Arc<ExternalLib>> = Vec::new(),
        ignore: Option<RegexPattern>,
        include: Option<RegexPattern>,
        let input_dir: PathBuf,
        let output_dir: PathBuf,
        let output_url: Option<UrlPath>,
    }
}

impl Config {
    pub fn parse(
        input_dir: PathBuf,
        output_dir: PathBuf,
        output_url: Option<UrlPath>,
    ) -> Result<Arc<Config>, String> {
        let mut config: Config = toml::from_str(
            &fs::read_to_string(input_dir.join("flash.toml"))
                .map_err(|e| format!("Unable to read flash.toml: {e}"))?,
        )
        .map_err(|e| format!("Unable to parse config: {e}"))?;

        config.input_dir = input_dir;
        config.output_dir = output_dir;
        config.output_url = output_url;
        Ok(Arc::from(config))
    }

    pub fn all_includes(&self) -> Vec<PathBuf> {
        self.sources
            .iter()
            .flat_map(|src| src.include.clone())
            .collect()
    }
}
