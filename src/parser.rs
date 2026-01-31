use nginx_config::parse_main;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("{0}")]
    ParseError(String),
}

pub fn parse_config(path: &Path) -> Result<nginx_config::ast::Main, ParseError> {
    let content = fs::read_to_string(path)?;
    parse_main(&content).map_err(|e| {
        // Extract just the human-readable part of the error
        let error_str = e.to_string();
        let clean_error = if let Some(idx) = error_str.find("Parse error at") {
            error_str[idx..].to_string()
        } else {
            error_str
        };
        ParseError::ParseError(clean_error)
    })
}
