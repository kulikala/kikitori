use crate::{AsrEngine, LoadOptions};

pub type EngineLoader = fn(&Weights, &LoadOptions) -> Result<Box<dyn AsrEngine>, String>;

pub struct EngineSpec {
    pub name: &'static str,
    pub hf_repo: &'static str,
    pub files: &'static [&'static str],
    pub load: EngineLoader,
}

#[derive(Clone, Debug, Default)]
pub struct Weights {
    pub paths: Vec<std::path::PathBuf>,
}

#[derive(Default)]
pub struct Registry {
    specs: Vec<&'static [EngineSpec]>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, specs: &'static [EngineSpec]) -> Self {
        self.specs.push(specs);
        self
    }

    pub fn lookup(&self, name: &str) -> Option<&EngineSpec> {
        let id = name.strip_prefix("hf:").unwrap_or(name);
        self.specs
            .iter()
            .flat_map(|group| group.iter())
            .find(|spec| spec.name == name || spec.name == id || spec.hf_repo == id)
    }

    pub fn specs(&self) -> impl Iterator<Item = &EngineSpec> + '_ {
        self.specs.iter().flat_map(|group| group.iter())
    }
}
