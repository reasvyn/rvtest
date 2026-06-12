use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Start building an architecture check.
pub fn arch_check() -> ArchCheck {
    ArchCheck { rules: Vec::new(), src_dir: PathBuf::from("src") }
}

// ---------------------------------------------------------------------------
// ArchCheck builder
// ---------------------------------------------------------------------------

pub struct ArchCheck {
    rules: Vec<Rule>,
    src_dir: PathBuf,
}

impl ArchCheck {
    /// Set a custom source directory (for non-standard crate layouts).
    pub fn src_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.src_dir = path.into();
        self
    }

    /// Select a module to attach rules to.
    pub fn module(mut self, name: &str) -> ModuleRuleBuilder {
        self.rules.push(Rule::Module {
            name: name.to_owned(),
            allowed_deps: None,
            forbidden_deps: None,
        });
        ModuleRuleBuilder { check: self, module_name: name.to_owned() }
    }

    /// Select the global set of rules that apply across all modules.
    pub fn all_modules(self) -> AllModulesRuleBuilder {
        AllModulesRuleBuilder { check: self }
    }

    /// Run all rules and panic on violations.
    pub fn assert_all_pass(self) {
        if let Err(msg) = self.run() {
            panic!("Architecture violations:\n{}", msg);
        }
    }

    /// Run all rules and return `Ok(())` or `Err(violations)`.
    pub fn run(mut self) -> Result<(), String> {
        let graph = DependencyGraph::from_dir(&self.src_dir)?;

        // Finalise module rules: no explicit rules → forbid nothing.
        for rule in &mut self.rules {
            if let Rule::Module { allowed_deps, forbidden_deps, .. } = rule {
                if allowed_deps.is_none() && forbidden_deps.is_none() {
                    *forbidden_deps = Some(HashSet::new());
                }
            }
        }

        let mut violations = Vec::new();

        for rule in &self.rules {
            match rule {
                Rule::Module { name, allowed_deps, forbidden_deps } => {
                    let deps = graph.dependencies_of(name);
                    if let Some(allowed) = allowed_deps {
                        for dep in &deps {
                            if !allowed.contains(dep) {
                                violations.push(format!(
                                    "  {} must not depend on {} (allowed: {})",
                                    name,
                                    dep,
                                    allowed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                                ));
                            }
                        }
                    }
                    if let Some(forbidden) = forbidden_deps {
                        for dep in &deps {
                            if forbidden.contains(dep) {
                                violations.push(format!(
                                    "  {} must not depend on {}",
                                    name, dep
                                ));
                            }
                        }
                    }
                }
                Rule::NoCycles => {
                    for cycle in &graph.find_cycles() {
                        violations.push(format!(
                            "  cycle detected: {}",
                            cycle.join(" → ")
                        ));
                    }
                }
                Rule::PublicDocs => {}
            }
        }

        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations.join("\n"))
        }
    }
}

// ---------------------------------------------------------------------------
// Module rule builder
// ---------------------------------------------------------------------------

pub struct ModuleRuleBuilder {
    check: ArchCheck,
    module_name: String,
}

impl ModuleRuleBuilder {
    fn find_mut(&mut self) -> &mut Rule {
        self.check
            .rules
            .iter_mut()
            .find(|r| matches!(r, Rule::Module { name, .. } if *name == self.module_name))
            .expect("module rule not found")
    }

    pub fn may_depend_on(mut self, deps: &[&str]) -> ArchCheck {
        let set: HashSet<String> = deps.iter().map(|s| s.to_string()).collect();
        if let Rule::Module { allowed_deps, .. } = self.find_mut() {
            *allowed_deps = Some(set);
        }
        self.check
    }

    pub fn may_not_depend_on(mut self, deps: &[&str]) -> ArchCheck {
        let set: HashSet<String> = deps.iter().map(|s| s.to_string()).collect();
        if let Rule::Module { forbidden_deps, .. } = self.find_mut() {
            let current = forbidden_deps.get_or_insert_with(HashSet::new);
            current.extend(set);
        }
        self.check
    }
}

impl From<ModuleRuleBuilder> for ArchCheck {
    fn from(b: ModuleRuleBuilder) -> Self {
        b.check
    }
}

// ---------------------------------------------------------------------------
// All-modules rule builder
// ---------------------------------------------------------------------------

pub struct AllModulesRuleBuilder {
    check: ArchCheck,
}

impl AllModulesRuleBuilder {
    pub fn must_not_have_cycles(mut self) -> ArchCheck {
        self.check.rules.push(Rule::NoCycles);
        self.check
    }

    pub fn public_api_doc_required(mut self) -> ArchCheck {
        self.check.rules.push(Rule::PublicDocs);
        self.check
    }
}

// ---------------------------------------------------------------------------
// Internal rule types
// ---------------------------------------------------------------------------

enum Rule {
    Module {
        name: String,
        allowed_deps: Option<HashSet<String>>,
        forbidden_deps: Option<HashSet<String>>,
    },
    NoCycles,
    PublicDocs,
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rule::Module { name, .. } => write!(f, "Module({})", name),
            Rule::NoCycles => write!(f, "NoCycles"),
            Rule::PublicDocs => write!(f, "PublicDocs"),
        }
    }
}

// ---------------------------------------------------------------------------
// Dependency graph
// ---------------------------------------------------------------------------

struct DependencyGraph {
    edges: HashMap<String, HashSet<String>>,
}

impl DependencyGraph {
    fn from_dir(dir: &Path) -> Result<Self, String> {
        if !dir.is_dir() {
            return Err(format!("directory not found: {:?}", dir));
        }

        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
        let mut files: Vec<PathBuf> = Vec::new();
        collect_rs_files(dir, &mut files, dir);

        for file in &files {
            let rel = file.strip_prefix(dir).map_err(|e| format!("path: {e}"))?;
            let module = path_to_module(rel);
            let content = std::fs::read_to_string(file)
                .map_err(|e| format!("read {:?}: {e}", file))?;
            let deps = parse_deps(&content);

            let entry: &mut HashSet<String> = edges.entry(module).or_default();
            for dep in deps {
                if dep.starts_with("crate::") {
                    entry.insert(dep.trim_start_matches("crate::").to_owned());
                }
            }
        }

        Ok(DependencyGraph { edges })
    }

    fn dependencies_of(&self, module: &str) -> HashSet<String> {
        self.edges.get(module).cloned().unwrap_or_default()
    }

    fn find_cycles(&self) -> Vec<Vec<String>> {
        let nodes: Vec<&String> = self.edges.keys().collect();
        let mut visited: HashSet<&String> = HashSet::new();
        let mut in_stack: HashSet<&String> = HashSet::new();
        let mut stack: Vec<&String> = Vec::new();
        let mut cycles: Vec<Vec<String>> = Vec::new();

        fn dfs<'a>(
            node: &'a String,
            graph: &'a HashMap<String, HashSet<String>>,
            visited: &mut HashSet<&'a String>,
            in_stack: &mut HashSet<&'a String>,
            stack: &mut Vec<&'a String>,
            cycles: &mut Vec<Vec<String>>,
        ) {
            if !visited.insert(node) {
                return;
            }
            in_stack.insert(node);
            stack.push(node);

            if let Some(deps) = graph.get(node) {
                for dep in deps {
                    if in_stack.contains(dep) {
                        let pos = stack.iter().position(|n| *n == dep).unwrap();
                        let cycle: Vec<String> = stack[pos..]
                            .iter()
                            .map(|s| (*s).clone())
                            .collect();
                        cycles.push(cycle);
                    } else {
                        dfs(dep, graph, visited, in_stack, stack, cycles);
                    }
                }
            }

            stack.pop();
            in_stack.remove(node);
        }

        for node in &nodes {
            dfs(node, &self.edges, &mut visited, &mut in_stack, &mut stack, &mut cycles);
        }

        cycles
    }
}

// ---------------------------------------------------------------------------
// File scanning helpers
// ---------------------------------------------------------------------------

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>, root: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') && name != "target" {
                collect_rs_files(&path, out, root);
            }
        } else if path.extension().map_or(false, |e| e == "rs") {
            out.push(path);
        }
    }
}

fn path_to_module(rel: &Path) -> String {
    let s = rel.to_string_lossy();
    let stem = s.strip_suffix(".rs").unwrap_or(&s);
    if stem.ends_with("/mod") || stem == "mod" {
        let parent = rel.parent().and_then(|p| p.to_str()).unwrap_or("");
        return if parent.is_empty() { "crate_root".into() } else { parent.replace('/', "::") };
    }
    stem.replace('/', "::")
}

fn parse_deps(content: &str) -> HashSet<String> {
    let mut deps = HashSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("use crate::") {
            let path = rest.trim_end_matches(';');
            let top = path.split("::").next().unwrap_or(path);
            if !top.is_empty() {
                deps.insert(top.to_owned());
            }
        }
        if let Some(rest) = trimmed.strip_prefix("pub mod ") {
            let name = rest.split(';').next().unwrap_or(rest).trim();
            deps.insert(name.to_owned());
        } else if let Some(rest) = trimmed.strip_prefix("mod ") {
            let name = rest.split(';').next().unwrap_or(rest).trim();
            deps.insert(name.to_owned());
        }
    }
    deps
}
