use std::collections::HashMap;
use std::path::{Path, PathBuf};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_ast::ast::*;

#[derive(Debug, Clone)]
pub struct ExportSource {
    pub source_path: PathBuf,
    pub original_name: Option<String>,
}

#[derive(Debug, Default)]
pub struct BarrelCache {
    exports: HashMap<PathBuf, HashMap<String, ExportSource>>,
    non_barrels: std::collections::HashSet<PathBuf>,
}

impl BarrelCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, barrel_path: &Path) -> Option<&HashMap<String, ExportSource>> {
        self.exports.get(barrel_path)
    }

    pub fn insert(&mut self, barrel_path: PathBuf, exports: HashMap<String, ExportSource>) {
        self.exports.insert(barrel_path, exports);
    }

    pub fn is_known_non_barrel(&self, path: &Path) -> bool {
        self.non_barrels.contains(path)
    }

    pub fn mark_non_barrel(&mut self, path: PathBuf) {
        self.non_barrels.insert(path);
    }

    pub fn is_barrel(&self, path: &Path) -> bool {
        self.exports.contains_key(path)
    }
}

pub struct BarrelAnalyzer {
    #[allow(dead_code)]
    root: PathBuf,
    cache: BarrelCache,
}

impl BarrelAnalyzer {
    pub fn new(root: PathBuf) -> Self {
        Self { root, cache: BarrelCache::new() }
    }

    pub fn check_and_cache_barrel(&mut self, resolved_path: &Path) -> bool {
        let canonical = std::fs::canonicalize(resolved_path).unwrap_or(resolved_path.to_path_buf());
        
        if self.cache.is_barrel(&canonical) {
            return true;
        }
        
        if self.cache.is_known_non_barrel(&canonical) {
            return false;
        }
        
        if self.is_barrel_file(&canonical) {
            let exports = self.analyze_barrel_uncached(&canonical);
            if !exports.is_empty() {
                self.cache.insert(canonical, exports);
                return true;
            }
        }
        
        self.cache.mark_non_barrel(canonical);
        false
    }

    pub fn resolve_barrel_imports(
        &mut self,
        barrel_path: &Path,
        imported_names: &[String],
    ) -> HashMap<String, PathBuf> {
        let canonical = std::fs::canonicalize(barrel_path).unwrap_or(barrel_path.to_path_buf());
        
        if !self.cache.is_barrel(&canonical) {
            let exports = self.analyze_barrel_uncached(&canonical);
            if exports.is_empty() {
                return HashMap::new();
            }
            self.cache.insert(canonical.clone(), exports);
        }
        
        let mut result = HashMap::new();
        
        if let Some(exports) = self.cache.get(&canonical) {
            for name in imported_names {
                if let Some(source) = exports.get(name) {
                    result.insert(name.clone(), source.source_path.clone());
                }
            }
        }
        
        result
    }

    pub fn get_cached_exports(&self, barrel_path: &Path) -> Option<&HashMap<String, ExportSource>> {
        let canonical = std::fs::canonicalize(barrel_path).unwrap_or(barrel_path.to_path_buf());
        self.cache.get(&canonical)
    }

    pub fn analyze_barrel(&mut self, barrel_path: &Path) -> HashMap<String, ExportSource> {
        let canonical = std::fs::canonicalize(barrel_path).unwrap_or(barrel_path.to_path_buf());
        
        if let Some(cached) = self.cache.get(&canonical) {
            return cached.clone();
        }
        
        let exports = self.analyze_barrel_uncached(&canonical);
        if !exports.is_empty() {
            self.cache.insert(canonical, exports.clone());
        }
        exports
    }

    fn analyze_barrel_uncached(&self, barrel_path: &Path) -> HashMap<String, ExportSource> {
        let mut exports: HashMap<String, ExportSource> = HashMap::new();
        let barrel_path = std::fs::canonicalize(barrel_path).unwrap_or(barrel_path.to_path_buf());
        
        let content = match std::fs::read_to_string(&barrel_path) {
            Ok(c) => c,
            Err(_) => return exports,
        };

        let allocator = Allocator::default();
        let source_type = SourceType::from_path(&barrel_path).unwrap_or_default();
        let parser = Parser::new(&allocator, &content, source_type);
        let result = parser.parse();

        if result.panicked {
            return exports;
        }

        let barrel_dir = barrel_path.parent().unwrap_or(Path::new("."));
        let barrel_canonical = barrel_path.clone();

        for stmt in &result.program.body {
            match stmt {
                Statement::ExportNamedDeclaration(decl) => {
                    if let Some(source) = &decl.source {
                        let source_path = self.resolve_path(barrel_dir, source.value.as_str());
                        
                        for spec in &decl.specifiers {
                            let exported_name = spec.exported.name().to_string();
                            let local_name = spec.local.name().to_string();
                            
                            exports.insert(exported_name, ExportSource {
                                source_path: source_path.clone(),
                                original_name: if local_name != spec.exported.name().as_str() {
                                    Some(local_name)
                                } else {
                                    None
                                },
                            });
                        }
                    } else if let Some(declaration) = &decl.declaration {
                        let names = self.get_declaration_names(declaration);
                        for name in names {
                            exports.insert(name, ExportSource {
                                source_path: barrel_canonical.clone(),
                                original_name: None,
                            });
                        }
                    }
                }
                Statement::ExportAllDeclaration(decl) => {
                    let source_path = self.resolve_path(barrel_dir, decl.source.value.as_str());
                    let nested_exports = self.analyze_barrel_uncached(&source_path);
                    
                    for (name, source) in nested_exports {
                        exports.insert(name, source);
                    }
                }
                Statement::ExportDefaultDeclaration(_) => {
                    exports.insert("default".to_string(), ExportSource {
                        source_path: barrel_canonical.clone(),
                        original_name: None,
                    });
                }
                _ => {}
            }
        }

        exports
    }
    
    fn get_declaration_names(&self, decl: &Declaration) -> Vec<String> {
        let mut names = Vec::new();
        
        match decl {
            Declaration::VariableDeclaration(var_decl) => {
                for declarator in &var_decl.declarations {
                    self.collect_binding_names(&declarator.id, &mut names);
                }
            }
            Declaration::FunctionDeclaration(func_decl) => {
                if let Some(id) = &func_decl.id {
                    names.push(id.name.to_string());
                }
            }
            Declaration::ClassDeclaration(class_decl) => {
                if let Some(id) = &class_decl.id {
                    names.push(id.name.to_string());
                }
            }
            Declaration::TSTypeAliasDeclaration(type_decl) => {
                names.push(type_decl.id.name.to_string());
            }
            Declaration::TSInterfaceDeclaration(iface_decl) => {
                names.push(iface_decl.id.name.to_string());
            }
            Declaration::TSEnumDeclaration(enum_decl) => {
                names.push(enum_decl.id.name.to_string());
            }
            _ => {}
        }
        
        names
    }
    
    fn collect_binding_names(&self, pattern: &BindingPattern, names: &mut Vec<String>) {
        match &pattern.kind {
            BindingPatternKind::BindingIdentifier(id) => {
                names.push(id.name.to_string());
            }
            BindingPatternKind::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.collect_binding_names(&prop.value, names);
                }
            }
            BindingPatternKind::ArrayPattern(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.collect_binding_names(elem, names);
                }
            }
            BindingPatternKind::AssignmentPattern(assign) => {
                self.collect_binding_names(&assign.left, names);
            }
        }
    }

    pub fn find_barrels(&self, dir: &Path) -> Vec<PathBuf> {
        let mut barrels = Vec::new();
        self.find_barrels_recursive(dir, &mut barrels);
        barrels
    }

    fn find_barrels_recursive(&self, dir: &Path, barrels: &mut Vec<PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "node_modules" && name != ".git" && !name.starts_with('.') {
                    self.find_barrels_recursive(&path, barrels);
                }
            } else if self.is_barrel_file(&path) {
                barrels.push(path);
            }
        }
    }

    fn is_barrel_file(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        
        if !matches!(name, "index.ts" | "index.tsx" | "index.js" | "index.jsx") {
            return false;
        }
        
        self.is_pure_barrel(path)
    }
    
    fn is_pure_barrel(&self, path: &Path) -> bool {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return false,
        };

        let allocator = Allocator::default();
        let source_type = SourceType::from_path(path).unwrap_or_default();
        let parser = Parser::new(&allocator, &content, source_type);
        let result = parser.parse();

        if result.panicked {
            return false;
        }

        let mut has_reexports = false;
        
        for stmt in &result.program.body {
            match stmt {
                Statement::ExportNamedDeclaration(decl) => {
                    if decl.source.is_some() {
                        has_reexports = true;
                    } else if decl.declaration.is_some() {
                        return false;
                    } else if !decl.specifiers.is_empty() {
                        continue;
                    }
                }
                Statement::ExportAllDeclaration(_) => {
                    has_reexports = true;
                }
                Statement::ExportDefaultDeclaration(decl) => {
                    match &decl.declaration {
                        ExportDefaultDeclarationKind::Identifier(_) => continue,
                        _ => return false,
                    }
                }
                Statement::ImportDeclaration(_) => continue,
                Statement::TSExportAssignment(_) |
                Statement::TSNamespaceExportDeclaration(_) => continue,
                _ => return false,
            }
        }
        
        has_reexports
    }

    fn resolve_path(&self, from_dir: &Path, import_path: &str) -> PathBuf {
        if !import_path.starts_with('.') {
            return PathBuf::from(import_path);
        }

        let base = from_dir.join(import_path);
        let normalized = self.normalize_path(&base);
        
        for ext in &[".ts", ".tsx", ".js", ".jsx"] {
            let with_ext = normalized.with_extension(ext.trim_start_matches('.'));
            if with_ext.exists() {
                return std::fs::canonicalize(&with_ext).unwrap_or(with_ext);
            }
        }

        for ext in &[".ts", ".tsx", ".js", ".jsx"] {
            let index = normalized.join(format!("index{}", ext));
            if index.exists() {
                return std::fs::canonicalize(&index).unwrap_or(index);
            }
        }

        std::fs::canonicalize(&normalized).unwrap_or(normalized)
    }

    fn normalize_path(&self, path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    if !components.is_empty() {
                        components.pop();
                    }
                }
                std::path::Component::CurDir => {}
                c => components.push(c),
            }
        }
        components.iter().collect()
    }
}
