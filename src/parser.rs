use nginx_config::parse_main;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse nginx config: {0}")]
    ParseError(String),
}

pub fn parse_config(path: &Path) -> Result<nginx_config::ast::Main, ParseError> {
    let content = fs::read_to_string(path)?;
    parse_main(&content).map_err(|e| ParseError::ParseError(e.to_string()))
}
