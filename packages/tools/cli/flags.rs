use std::collections::BTreeMap;

pub struct Flags {
    name: String,
    values: BTreeMap<String, String>,
    booleans: BTreeMap<String, bool>,
    multiple: BTreeMap<String, Vec<String>>,
    pub rest: Vec<String>,
}

impl Flags {
    pub fn new(name: &str) -> Flags {
        Flags {
            name: name.into(),
            values: BTreeMap::new(),
            booleans: BTreeMap::new(),
            multiple: BTreeMap::new(),
            rest: Vec::new(),
        }
    }

    pub fn string(mut self, name: &str, fallback: &str) -> Flags {
        self.values.insert(name.into(), fallback.into());
        self
    }

    pub fn boolean(mut self, name: &str) -> Flags {
        self.booleans.insert(name.into(), false);
        self
    }

    pub fn parse(mut self, arguments: &[String]) -> Result<Flags, String> {
        let mut index = 0;
        while index < arguments.len() {
            let argument = &arguments[index];
            index += 1;
            if argument == "--" {
                self.rest.extend(arguments[index..].iter().cloned());
                break;
            }
            let Some(stripped) = argument
                .strip_prefix("--")
                .or_else(|| argument.strip_prefix('-'))
                .filter(|stripped| !stripped.is_empty() && argument != "-")
            else {
                self.rest.push(argument.clone());
                continue;
            };
            let (name, inline) = stripped
                .split_once('=')
                .map_or((stripped, None), |(name, value)| {
                    (name, Some(value.to_owned()))
                });
            if let Some(flag) = self.booleans.get_mut(name) {
                *flag = inline
                    .as_deref()
                    .is_none_or(|value| value == "true" || value == "1");
                continue;
            }
            let value = match inline {
                Some(value) => value,
                None => {
                    let Some(value) = arguments.get(index) else {
                        return Err(format!("flag needs an argument: -{name}"));
                    };
                    index += 1;
                    value.clone()
                }
            };
            if let Some(list) = self.multiple.get_mut(name) {
                list.push(value);
            } else if let Some(slot) = self.values.get_mut(name) {
                *slot = value;
            } else {
                return Err(format!(
                    "flag provided but not defined: -{name} (astral {})",
                    self.name
                ));
            }
        }
        Ok(self)
    }

    pub fn get(&self, name: &str) -> String {
        self.values.get(name).cloned().unwrap_or_default()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.booleans.get(name).copied().unwrap_or(false)
    }
}

pub fn environment(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
