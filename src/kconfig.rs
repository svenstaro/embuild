/// A quick and dirty parser for the .config files generated by kconfig systems like
/// the ESP-IDF one
use std::{
    convert::TryFrom,
    env,
    fmt::Display,
    fs,
    io::{self, BufRead},
    path::Path,
};

use anyhow::*;

use crate::cargo;

const VAR_CFG_ARGS_KEY: &str = "EMBUILD_CFG_ARGS";

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Tristate {
    True,
    False,
    Module,
    NotSet,
}

#[derive(Clone, Debug)]
pub enum Value {
    Tristate(Tristate),
    String(String),
}

impl Value {
    fn parse(str: impl AsRef<str>) -> Option<Self> {
        let str = str.as_ref();

        Some(if str.starts_with('\"') {
            Self::String(str.to_owned()) // TODO: Properly parse and escape
        } else if str == "y" {
            Self::Tristate(Tristate::True)
        } else if str == "n" {
            Self::Tristate(Tristate::False)
        } else if str == "m" {
            Self::Tristate(Tristate::Module)
        } else {
            return None;
        })
    }
}

pub fn load(path: impl AsRef<Path>) -> Result<impl Iterator<Item = (String, Value)>> {
    Ok(io::BufReader::new(fs::File::open(path.as_ref())?)
        .lines()
        .filter_map(|line| line.ok().map(|l| l.trim().to_owned()))
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut split = line.split('=');

            if let Some(key) = split.next() {
                split
                    .next()
                    .map(|v| v.trim())
                    .map(Value::parse)
                    .flatten()
                    .map(|value| (key.to_owned(), value))
            } else {
                None
            }
        }))
}

#[derive(Clone, Debug)]
pub struct CfgArgs(Vec<(String, Value)>);

impl TryFrom<&Path> for CfgArgs {
    type Error = anyhow::Error;

    fn try_from(path: &Path) -> Result<Self> {
        Ok(Self(load(path)?.collect()))
    }
}

impl CfgArgs {
    /// Add configuration options from the parsed kconfig output file.
    ///
    /// All options will consist of `<prefix>_<option name>` where both the prefix and the option name are
    /// automatically lowercased.
    ///
    /// They can be used in conditional compilation using the `#[cfg()]` attribute or the
    /// `cfg!()` macro (ex. `cfg!(<prefix>_<kconfig option>)`).
    pub fn output(&self, prefix: impl AsRef<str>) {
        for arg in self.gather(prefix) {
            cargo::set_rustc_cfg(arg, "");
        }
    }

    /// Propagate all configuration options to all dependents of this crate.
    ///
    /// All options will consist of `<prefix>_<option name>` where the option name is
    /// automatically lowercased.
    ///
    /// ### **Important**
    /// Calling this method in a dependency doesn't do anything on itself. All dependents
    /// that want to have these options propagated must call
    /// [`CfgArgs::output_propagated`] in their build script with the value of this
    /// crate's `links` property (specified in `Cargo.toml`).
    pub fn propagate(&self, prefix: impl AsRef<str>) {
        let args = self.gather(prefix);

        cargo::set_metadata(VAR_CFG_ARGS_KEY, args.join(":"));
    }

    /// Add options from `lib_name` which have been propagated using [`propagate`](CfgArgs::propagate).
    ///
    /// `lib_name` doesn't refer to a crate, library or package name, it refers to a
    /// dependency's `links` property value, which is specified in its package manifest
    /// (`Cargo.toml`).
    pub fn output_propagated(lib_name: impl Display) -> Result<()> {
        for arg in env::var(format!("DEP_{}_{}", lib_name, VAR_CFG_ARGS_KEY))?.split(':') {
            cargo::set_rustc_cfg(arg, "");
        }
        Ok(())
    }

    pub fn gather(&self, prefix: impl AsRef<str>) -> Vec<String> {
        self.0
            .iter()
            .filter_map(|(key, value)| match value {
                Value::Tristate(Tristate::True) => Some(format!(
                    "{}_{}",
                    prefix.as_ref().to_lowercase(),
                    key.to_lowercase()
                )),
                _ => None,
            })
            .collect()
    }
}
