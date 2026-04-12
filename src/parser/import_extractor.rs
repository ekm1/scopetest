use std::path::Path;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_ast::ast::*;

use super::ParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportType {
    StaticImport,
    DynamicImport,
    Require,
    ReExport,
}

#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub source: String,
    pub import_type: ImportType,
    /// Named imports (e.g., `{ Button, Input }` -> ["Button", "Input"])
    pub named_imports: Vec<String>,
    /// Whether this is a namespace import (import * as X)
    pub is_namespace: bool,
    /// Whether this has a default import
    pub has_default: bool,
}

fn extract_imports_from_program(program: &Program) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    let mut namespace_locals: Vec<(String, usize)> = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                let mut named_imports = Vec::new();
                let mut is_namespace = false;
                let mut has_default = false;
                let mut namespace_local_name: Option<String> = None;
                
                if let Some(specifiers) = &decl.specifiers {
                    for spec in specifiers {
                        match spec {
                            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                                named_imports.push(s.imported.name().to_string());
                            }
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {
                                has_default = true;
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) => {
                                is_namespace = true;
                                namespace_local_name = Some(ns.local.name.to_string());
                            }
                        }
                    }
                }
                
                let idx = imports.len();
                imports.push(ImportInfo {
                    source: decl.source.value.to_string(),
                    import_type: ImportType::StaticImport,
                    named_imports,
                    is_namespace,
                    has_default,
                });
                
                if let Some(local) = namespace_local_name {
                    namespace_locals.push((local, idx));
                }
            }
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(source) = &decl.source {
                    let named_imports: Vec<String> = decl.specifiers
                        .iter()
                        .map(|s| s.local.name().to_string())
                        .collect();
                    
                    imports.push(ImportInfo {
                        source: source.value.to_string(),
                        import_type: ImportType::ReExport,
                        named_imports,
                        is_namespace: false,
                        has_default: false,
                    });
                }
            }
            Statement::ExportAllDeclaration(decl) => {
                imports.push(ImportInfo {
                    source: decl.source.value.to_string(),
                    import_type: ImportType::ReExport,
                    named_imports: vec![],
                    is_namespace: true, // export * is like namespace
                    has_default: false,
                });
            }
            Statement::ExpressionStatement(expr_stmt) => {
                extract_imports_from_expression(&expr_stmt.expression, &mut imports);
            }
            Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    if let Some(init) = &decl.init {
                        extract_imports_from_expression(init, &mut imports);
                    }
                }
            }
            _ => {}
        }
    }


    if !namespace_locals.is_empty() {
        let mut usages: Vec<(usize, Vec<String>)> = namespace_locals
            .iter()
            .map(|(_, idx)| (*idx, Vec::new()))
            .collect();
        
        collect_namespace_usages_from_program(program, &namespace_locals, &mut usages);
        
        for (idx, names) in usages {
            if !names.is_empty() {
                imports[idx].named_imports = names;
                imports[idx].is_namespace = false;
            }
        }
    }

    imports
}

fn collect_namespace_usages_from_program(
    program: &Program,
    namespace_locals: &[(String, usize)],
    usages: &mut [(usize, Vec<String>)],
) {
    for stmt in &program.body {
        collect_namespace_usages_from_stmt(stmt, namespace_locals, usages);
    }
    for (_, names) in usages.iter_mut() {
        names.sort();
        names.dedup();
    }
}

fn collect_namespace_usages_from_stmt(
    stmt: &Statement,
    ns: &[(String, usize)],
    usages: &mut [(usize, Vec<String>)],
) {
    match stmt {
        Statement::ExpressionStatement(e) => collect_namespace_usages_from_expr(&e.expression, ns, usages),
        Statement::VariableDeclaration(v) => {
            for decl in &v.declarations {
                if let Some(init) = &decl.init {
                    collect_namespace_usages_from_expr(init, ns, usages);
                }
            }
        }
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                collect_namespace_usages_from_expr(arg, ns, usages);
            }
        }
        Statement::IfStatement(i) => {
            collect_namespace_usages_from_expr(&i.test, ns, usages);
            collect_namespace_usages_from_stmt(&i.consequent, ns, usages);
            if let Some(alt) = &i.alternate {
                collect_namespace_usages_from_stmt(alt, ns, usages);
            }
        }
        Statement::BlockStatement(b) => {
            for s in &b.body {
                collect_namespace_usages_from_stmt(s, ns, usages);
            }
        }
        Statement::ForStatement(f) => {
            if let Some(body) = Some(&f.body) {
                collect_namespace_usages_from_stmt(body, ns, usages);
            }
        }
        Statement::WhileStatement(w) => {
            collect_namespace_usages_from_expr(&w.test, ns, usages);
            collect_namespace_usages_from_stmt(&w.body, ns, usages);
        }
        Statement::SwitchStatement(s) => {
            collect_namespace_usages_from_expr(&s.discriminant, ns, usages);
            for case in &s.cases {
                for stmt in &case.consequent {
                    collect_namespace_usages_from_stmt(stmt, ns, usages);
                }
            }
        }
        Statement::TryStatement(t) => {
            for s in &t.block.body {
                collect_namespace_usages_from_stmt(s, ns, usages);
            }
            if let Some(handler) = &t.handler {
                for s in &handler.body.body {
                    collect_namespace_usages_from_stmt(s, ns, usages);
                }
            }
            if let Some(finalizer) = &t.finalizer {
                for s in &finalizer.body {
                    collect_namespace_usages_from_stmt(s, ns, usages);
                }
            }
        }
        _ => {}
    }
}

fn collect_namespace_usages_from_expr(
    expr: &Expression,
    ns: &[(String, usize)],
    usages: &mut [(usize, Vec<String>)],
) {
    match expr {
        Expression::StaticMemberExpression(m) => {
            if let Expression::Identifier(obj) = &m.object {
                let obj_name = obj.name.as_str();
                for (i, (local, _)) in ns.iter().enumerate() {
                    if local == obj_name {
                        usages[i].1.push(m.property.name.to_string());
                    }
                }
            }
            // Also recurse into the object in case of chaining
            collect_namespace_usages_from_expr(&m.object, ns, usages);
        }
        Expression::ComputedMemberExpression(m) => {
            collect_namespace_usages_from_expr(&m.object, ns, usages);
            collect_namespace_usages_from_expr(&m.expression, ns, usages);
        }
        Expression::CallExpression(c) => {
            collect_namespace_usages_from_expr(&c.callee, ns, usages);
            
            if let Expression::StaticMemberExpression(callee_member) = &c.callee {
                if callee_member.property.name == "spyOn" {
                    if c.arguments.len() >= 2 {
                        if let Some(Expression::Identifier(obj_ident)) = c.arguments[0].as_expression() {
                            if let Some(Expression::StringLiteral(prop_lit)) = c.arguments[1].as_expression() {
                                let obj_name = obj_ident.name.as_str();
                                for (i, (local, _)) in ns.iter().enumerate() {
                                    if local == obj_name {
                                        usages[i].1.push(prop_lit.value.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            for arg in &c.arguments {
                match arg {
                    Argument::SpreadElement(s) => collect_namespace_usages_from_expr(&s.argument, ns, usages),
                    _ => {
                        if let Some(expr) = arg.as_expression() {
                            collect_namespace_usages_from_expr(expr, ns, usages);
                        }
                    }
                }
            }
        }
        Expression::ArrowFunctionExpression(a) => {
            for s in &a.body.statements {
                collect_namespace_usages_from_stmt(s, ns, usages);
            }
        }
        Expression::FunctionExpression(f) => {
            if let Some(body) = &f.body {
                for s in &body.statements {
                    collect_namespace_usages_from_stmt(s, ns, usages);
                }
            }
        }
        Expression::ConditionalExpression(c) => {
            collect_namespace_usages_from_expr(&c.test, ns, usages);
            collect_namespace_usages_from_expr(&c.consequent, ns, usages);
            collect_namespace_usages_from_expr(&c.alternate, ns, usages);
        }
        Expression::BinaryExpression(b) => {
            collect_namespace_usages_from_expr(&b.left, ns, usages);
            collect_namespace_usages_from_expr(&b.right, ns, usages);
        }
        Expression::LogicalExpression(l) => {
            collect_namespace_usages_from_expr(&l.left, ns, usages);
            collect_namespace_usages_from_expr(&l.right, ns, usages);
        }
        Expression::UnaryExpression(u) => {
            collect_namespace_usages_from_expr(&u.argument, ns, usages);
        }
        Expression::AssignmentExpression(a) => {
            // Scan left side for patterns like (namespace.prop as Type) = value
            match &a.left {
                AssignmentTarget::StaticMemberExpression(m) => {
                    if let Expression::Identifier(obj) = &m.object {
                        let obj_name = obj.name.as_str();
                        for (i, (local, _)) in ns.iter().enumerate() {
                            if local == obj_name {
                                usages[i].1.push(m.property.name.to_string());
                            }
                        }
                    }
                }
                AssignmentTarget::TSAsExpression(ts) => {
                    collect_namespace_usages_from_expr(&ts.expression, ns, usages);
                }
                AssignmentTarget::TSSatisfiesExpression(ts) => {
                    collect_namespace_usages_from_expr(&ts.expression, ns, usages);
                }
                AssignmentTarget::TSNonNullExpression(ts) => {
                    collect_namespace_usages_from_expr(&ts.expression, ns, usages);
                }
                _ => {}
            }
            collect_namespace_usages_from_expr(&a.right, ns, usages);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                collect_namespace_usages_from_expr(e, ns, usages);
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                collect_namespace_usages_from_expr(e, ns, usages);
            }
        }
        Expression::ObjectExpression(o) => {
            for prop in &o.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        collect_namespace_usages_from_expr(&p.value, ns, usages);
                    }
                    ObjectPropertyKind::SpreadProperty(s) => {
                        collect_namespace_usages_from_expr(&s.argument, ns, usages);
                    }
                }
            }
        }
        Expression::ArrayExpression(a) => {
            for elem in &a.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(s) => {
                        collect_namespace_usages_from_expr(&s.argument, ns, usages);
                    }
                    _ => {
                        if let Some(expr) = elem.as_expression() {
                            collect_namespace_usages_from_expr(expr, ns, usages);
                        }
                    }
                }
            }
        }
        Expression::AwaitExpression(a) => {
            collect_namespace_usages_from_expr(&a.argument, ns, usages);
        }
        Expression::ParenthesizedExpression(p) => {
            collect_namespace_usages_from_expr(&p.expression, ns, usages);
        }
        Expression::TSAsExpression(t) => {
            collect_namespace_usages_from_expr(&t.expression, ns, usages);
        }
        Expression::TSNonNullExpression(t) => {
            collect_namespace_usages_from_expr(&t.expression, ns, usages);
        }
        _ => {}
    }
}

fn extract_imports_from_expression(expr: &Expression, imports: &mut Vec<ImportInfo>) {
    match expr {
        Expression::ImportExpression(import_expr) => {
            if let Expression::StringLiteral(lit) = &import_expr.source {
                imports.push(ImportInfo {
                    source: lit.value.to_string(),
                    import_type: ImportType::DynamicImport,
                    named_imports: vec![],
                    is_namespace: true, // dynamic imports are like namespace
                    has_default: false,
                });
            }
        }
        Expression::CallExpression(call_expr) => {
            if let Expression::Identifier(ident) = &call_expr.callee {
                if ident.name == "require" {
                    if let Some(arg) = call_expr.arguments.first() {
                        if let Argument::StringLiteral(lit) = arg {
                            imports.push(ImportInfo {
                                source: lit.value.to_string(),
                                import_type: ImportType::Require,
                                named_imports: vec![],
                                is_namespace: true, // require is like namespace
                                has_default: false,
                            });
                        }
                    }
                }
            }
            for arg in &call_expr.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    extract_imports_from_expression(&spread.argument, imports);
                }
            }
        }
        Expression::AwaitExpression(await_expr) => {
            extract_imports_from_expression(&await_expr.argument, imports);
        }
        _ => {}
    }
}

pub fn parse_file(path: &Path) -> Result<Vec<ImportInfo>, ParseError> {
    let source_text = std::fs::read_to_string(path)?;
    parse_source(&source_text, path)
}

pub fn parse_source(source: &str, path: &Path) -> Result<Vec<ImportInfo>, ParseError> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    
    if result.panicked {
        return Err(ParseError::SyntaxError(
            result.errors.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    
    Ok(extract_imports_from_program(&result.program))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_ts(source: &str) -> Vec<ImportInfo> {
        parse_source(source, &PathBuf::from("test.ts")).unwrap()
    }

    #[test]
    fn test_static_import() {
        let imports = parse_ts(r#"import { foo } from './foo';"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
        assert_eq!(imports[0].import_type, ImportType::StaticImport);
    }

    #[test]
    fn test_default_import() {
        let imports = parse_ts(r#"import foo from './foo';"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
    }

    #[test]
    fn test_namespace_import() {
        let imports = parse_ts(r#"import * as foo from './foo';"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
    }

    #[test]
    fn test_namespace_import_with_usage() {
        let imports = parse_ts(r#"
            import * as utils from './utils';
            const x = utils.Device;
            utils.setDevice('mobile');
            const y = utils.type();
        "#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./utils");
        assert!(!imports[0].is_namespace, "should be converted to named imports");
        assert!(imports[0].named_imports.contains(&"Device".to_string()));
        assert!(imports[0].named_imports.contains(&"setDevice".to_string()));
        assert!(imports[0].named_imports.contains(&"type".to_string()));
    }

    #[test]
    fn test_namespace_import_no_usage_stays_namespace() {
        let imports = parse_ts(r#"
            import * as utils from './utils';
            console.log("no usage of utils");
        "#);
        assert_eq!(imports.len(), 1);
        assert!(imports[0].is_namespace, "should stay namespace when no member access found");
    }

    #[test]
    fn test_namespace_import_jest_spyon() {
        let imports = parse_ts(r#"
            import * as utils from './utils';
            const spy = jest.spyOn(utils, "callTest");
        "#);
        assert_eq!(imports.len(), 1);
        assert!(!imports[0].is_namespace, "should be converted to named imports");
        assert!(imports[0].named_imports.contains(&"callTest".to_string()));
    }

    #[test]
    fn test_dynamic_import() {
        let imports = parse_ts(r#"const foo = import('./foo');"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
        assert_eq!(imports[0].import_type, ImportType::DynamicImport);
    }

    #[test]
    fn test_require() {
        let imports = parse_ts(r#"const foo = require('./foo');"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
        assert_eq!(imports[0].import_type, ImportType::Require);
    }

    #[test]
    fn test_re_export_named() {
        let imports = parse_ts(r#"export { foo } from './foo';"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
        assert_eq!(imports[0].import_type, ImportType::ReExport);
    }

    #[test]
    fn test_re_export_all() {
        let imports = parse_ts(r#"export * from './foo';"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./foo");
        assert_eq!(imports[0].import_type, ImportType::ReExport);
    }

    #[test]
    fn test_multiple_imports() {
        let imports = parse_ts(r#"
            import { a } from './a';
            import b from './b';
            const c = require('./c');
            export * from './d';
        "#);
        assert_eq!(imports.len(), 4);
    }

    #[test]
    fn test_parent_dir_import() {
        let imports = parse_ts(r#"import { AppPopUps } from "..";"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "..");
        assert_eq!(imports[0].import_type, ImportType::StaticImport);
    }
}