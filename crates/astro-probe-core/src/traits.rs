use crate::facts::Fact;
use std::path::Path;

pub trait SourceParser {
    type Error: std::error::Error + Send + Sync + 'static;

    fn parse_project(&self, project_path: &Path) -> Result<Vec<Fact>, Self::Error>;
}

pub trait TypeSystem {
    type Error: std::error::Error + Send + Sync + 'static;

    fn is_subtype(&self, sub: &str, sup: &str) -> Result<bool, Self::Error>;
}

pub trait DependencyAnalyzer<Conn> {
    type Error: std::error::Error + Send + Sync + 'static;

    fn analyze_dependency(
        &self,
        path: &Path,
        local_conn: &mut Conn,
        workspace_id: &str,
    ) -> Result<(), Self::Error>;
}

pub trait FrameworkAnalyzer<Conn> {
    type Error: std::error::Error + Send + Sync + 'static;

    fn analyze(&self, conn: &mut Conn) -> Result<(), Self::Error>;
}

pub trait LanguageFrontend<Conn> {
    type Error: std::error::Error + Send + Sync + 'static;

    fn process_project(
        &self,
        project_path: &Path,
        conn: &mut Conn,
        workspace_id: &str,
    ) -> Result<(), Self::Error>;
}
