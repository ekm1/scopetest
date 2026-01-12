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
}

fn extract_imports_from_program(program: &Program) -> Vec<ImportInfo> {
    let mut imports = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                imports.push(ImportInfo {
                    source: decl.source.value.to_string(),
                    import_type: ImportType::StaticImport,
                });
            }
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(source) = &decl.source {
                    imports.push(ImportInfo {
                        source: source.value.to_string(),
                        import_type: ImportType::ReExport,
                    });
                }
            }
            Statement::ExportAllDeclaration(decl) => {
                imports.push(ImportInfo {
                    source: decl.source.value.to_string(),
                    import_type: ImportType::ReExport,
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

    imports
}

fn extract_imports_from_expression(expr: &Expression, imports: &mut Vec<ImportInfo>) {
    match expr {
        Expression::ImportExpression(import_expr) => {
            if let Expression::StringLiteral(lit) = &import_expr.source {
                imports.push(ImportInfo {
                    source: lit.value.to_string(),
                    import_type: ImportType::DynamicImport,
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
}
