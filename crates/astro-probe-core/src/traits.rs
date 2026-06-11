use crate::facts::Fact;
use rusqlite::Connection;
use std::path::Path;

pub trait SourceParser {
    type Error: std::error::Error + Send + Sync + 'static;

    fn parse_project(&self, project_path: &Path) -> Result<Vec<Fact>, Self::Error>;
}

pub trait TypeSystem {
    type Error: std::error::Error + Send + Sync + 'static;

    fn is_subtype(&self, sub: &str, sup: &str) -> Result<bool, Self::Error>;
}

pub trait DependencyAnalyzer {
    type Error: std::error::Error + Send + Sync + 'static;

    fn analyze_dependency(
        &self,
        path: &Path,
        local_conn: &Connection,
        workspace_id: &str,
    ) -> Result<(), Self::Error>;
}

pub trait FrameworkAnalyzer {
    type Error: std::error::Error + Send + Sync + 'static;

    fn analyze(&self, conn: &Connection) -> Result<(), Self::Error>;
}

pub trait LanguageFrontend {
    type Error: std::error::Error + Send + Sync + 'static;

    fn process_project(
        &self,
        project_path: &Path,
        conn: &Connection,
        workspace_id: &str,
    ) -> Result<(), Self::Error>;
}
