//! Bulk Import Functionality
//!
//! Provides import capabilities for:
//! - CSV file import
//! - OSCAL (Open Security Controls Assessment Language) format
//! - JSON/JSONL import
//! - Streaming import for large files

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Supported import formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    /// Comma-separated values
    Csv,
    /// JSON array
    Json,
    /// JSON Lines (newline-delimited JSON)
    JsonL,
    /// OSCAL catalog format
    OscalCatalog,
    /// OSCAL SSP (System Security Plan) format
    OscalSsp,
    /// OSCAL component definition
    OscalComponent,
}

impl ImportFormat {
    /// Detect format from file extension
    pub fn from_extension(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            "jsonl" | "ndjson" => Some(Self::JsonL),
            _ => None,
        }
    }

    /// Detect OSCAL format from content
    pub fn detect_oscal(content: &str) -> Option<Self> {
        if content.contains("\"catalog\"") {
            Some(Self::OscalCatalog)
        } else if content.contains("\"system-security-plan\"") {
            Some(Self::OscalSsp)
        } else if content.contains("\"component-definition\"") {
            Some(Self::OscalComponent)
        } else {
            None
        }
    }
}

/// Result of a single import record
#[derive(Debug, Clone)]
pub struct ImportRecord {
    /// Line/row number in source
    pub line_number: usize,
    /// Original data as key-value pairs
    pub data: HashMap<String, serde_json::Value>,
    /// Any validation errors
    pub errors: Vec<String>,
    /// Whether the record is valid
    pub valid: bool,
}

impl ImportRecord {
    pub fn new(line_number: usize, data: HashMap<String, serde_json::Value>) -> Self {
        Self {
            line_number,
            data,
            errors: Vec::new(),
            valid: true,
        }
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.errors.push(error.into());
        self.valid = false;
        self
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.data.get(key).and_then(|v| v.as_str().map(String::from))
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.data.get(key).and_then(|v| v.as_i64())
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.data.get(key).and_then(|v| v.as_f64())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.data.get(key).and_then(|v| v.as_bool())
    }
}

/// Import job status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Pending,
    Validating,
    Importing,
    Completed,
    Failed,
    PartialSuccess,
}

/// Statistics for an import job
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportStats {
    pub total_records: usize,
    pub valid_records: usize,
    pub invalid_records: usize,
    pub imported_records: usize,
    pub skipped_records: usize,
    pub errors: Vec<ImportError>,
}

impl ImportStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_records == 0 {
            0.0
        } else {
            self.imported_records as f64 / self.total_records as f64
        }
    }
}

/// Import job configuration
#[derive(Debug, Clone)]
pub struct ImportConfig {
    /// Whether to skip invalid records or fail the entire import
    pub skip_invalid: bool,
    /// Maximum number of errors before aborting
    pub max_errors: usize,
    /// Batch size for processing
    pub batch_size: usize,
    /// Whether to perform a dry run (validation only)
    pub dry_run: bool,
    /// Column/field mapping for CSV imports
    pub field_mapping: Option<HashMap<String, String>>,
    /// Default values for missing fields
    pub defaults: HashMap<String, serde_json::Value>,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            skip_invalid: true,
            max_errors: 100,
            batch_size: 100,
            dry_run: false,
            field_mapping: None,
            defaults: HashMap::new(),
        }
    }
}

/// An import error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportError {
    pub line_number: Option<usize>,
    pub field: Option<String>,
    pub message: String,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Warning,
    Error,
    Fatal,
}

/// Import job representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportJob {
    pub id: Uuid,
    pub status: ImportStatus,
    pub format: ImportFormat,
    pub filename: Option<String>,
    pub org_id: String,
    pub user_id: String,
    pub stats: ImportStats,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ImportJob {
    pub fn new(format: ImportFormat, org_id: String, user_id: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: ImportStatus::Pending,
            format,
            filename: None,
            org_id,
            user_id,
            stats: ImportStats::default(),
            created_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

/// CSV import parser
pub struct CsvImporter {
    config: ImportConfig,
}

impl Default for CsvImporter {
    fn default() -> Self {
        Self::new(ImportConfig::default())
    }
}

impl CsvImporter {
    pub fn new(config: ImportConfig) -> Self {
        Self { config }
    }

    /// Parse CSV content and return records
    pub fn parse<R: Read>(&self, reader: R) -> Result<Vec<ImportRecord>, ImportError> {
        let mut csv_reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_reader(reader);

        let headers: Vec<String> = csv_reader
            .headers()
            .map_err(|e| ImportError {
                line_number: Some(1),
                field: None,
                message: format!("Failed to read headers: {}", e),
                severity: ErrorSeverity::Fatal,
            })?
            .iter()
            .map(String::from)
            .collect();

        let mut records = Vec::new();
        let mut line_number = 1;

        for result in csv_reader.records() {
            line_number += 1;

            match result {
                Ok(record) => {
                    let mut data = HashMap::new();

                    for (i, value) in record.iter().enumerate() {
                        if let Some(header) = headers.get(i) {
                            let key = if let Some(ref mapping) = self.config.field_mapping {
                                mapping.get(header).cloned().unwrap_or_else(|| header.clone())
                            } else {
                                header.clone()
                            };

                            // Try to parse as JSON value, fall back to string
                            let json_value = serde_json::from_str(value)
                                .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));

                            data.insert(key, json_value);
                        }
                    }

                    // Apply defaults
                    for (key, value) in &self.config.defaults {
                        data.entry(key.clone()).or_insert_with(|| value.clone());
                    }

                    records.push(ImportRecord::new(line_number, data));
                }
                Err(e) => {
                    let record = ImportRecord::new(line_number, HashMap::new())
                        .with_error(format!("Parse error: {}", e));
                    records.push(record);
                }
            }
        }

        Ok(records)
    }
}

/// JSON/JSONL import parser
pub struct JsonImporter {
    config: ImportConfig,
}

impl Default for JsonImporter {
    fn default() -> Self {
        Self::new(ImportConfig::default())
    }
}

impl JsonImporter {
    pub fn new(config: ImportConfig) -> Self {
        Self { config }
    }

    /// Parse JSON array content
    pub fn parse_json<R: Read>(&self, reader: R) -> Result<Vec<ImportRecord>, ImportError> {
        let array: Vec<serde_json::Value> = serde_json::from_reader(reader)
            .map_err(|e| ImportError {
                line_number: None,
                field: None,
                message: format!("Invalid JSON: {}", e),
                severity: ErrorSeverity::Fatal,
            })?;

        let mut records = Vec::new();

        for (i, value) in array.into_iter().enumerate() {
            let line_number = i + 1;

            match value {
                serde_json::Value::Object(obj) => {
                    let mut data: HashMap<String, serde_json::Value> = obj.into_iter().collect();

                    // Apply defaults
                    for (key, default) in &self.config.defaults {
                        data.entry(key.clone()).or_insert_with(|| default.clone());
                    }

                    records.push(ImportRecord::new(line_number, data));
                }
                _ => {
                    let record = ImportRecord::new(line_number, HashMap::new())
                        .with_error("Expected object, got different type");
                    records.push(record);
                }
            }
        }

        Ok(records)
    }

    /// Parse JSONL (newline-delimited JSON) content
    pub fn parse_jsonl<R: Read>(&self, reader: R) -> Result<Vec<ImportRecord>, ImportError> {
        let buf_reader = BufReader::new(reader);
        let mut records = Vec::new();

        for (i, line_result) in buf_reader.lines().enumerate() {
            let line_number = i + 1;

            match line_result {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(serde_json::Value::Object(obj)) => {
                            let mut data: HashMap<String, serde_json::Value> =
                                obj.into_iter().collect();

                            for (key, default) in &self.config.defaults {
                                data.entry(key.clone()).or_insert_with(|| default.clone());
                            }

                            records.push(ImportRecord::new(line_number, data));
                        }
                        Ok(_) => {
                            let record = ImportRecord::new(line_number, HashMap::new())
                                .with_error("Expected JSON object");
                            records.push(record);
                        }
                        Err(e) => {
                            let record = ImportRecord::new(line_number, HashMap::new())
                                .with_error(format!("Invalid JSON: {}", e));
                            records.push(record);
                        }
                    }
                }
                Err(e) => {
                    let record = ImportRecord::new(line_number, HashMap::new())
                        .with_error(format!("Read error: {}", e));
                    records.push(record);
                }
            }
        }

        Ok(records)
    }
}

/// OSCAL import parser
pub struct OscalImporter {
    config: ImportConfig,
}

impl Default for OscalImporter {
    fn default() -> Self {
        Self::new(ImportConfig::default())
    }
}

impl OscalImporter {
    pub fn new(config: ImportConfig) -> Self {
        Self { config }
    }

    /// Parse OSCAL catalog format
    pub fn parse_catalog<R: Read>(&self, reader: R) -> Result<Vec<ImportRecord>, ImportError> {
        let doc: serde_json::Value = serde_json::from_reader(reader)
            .map_err(|e| ImportError {
                line_number: None,
                field: None,
                message: format!("Invalid OSCAL JSON: {}", e),
                severity: ErrorSeverity::Fatal,
            })?;

        let mut records = Vec::new();
        let mut line_number = 0;

        // Navigate to controls
        if let Some(catalog) = doc.get("catalog") {
            if let Some(groups) = catalog.get("groups").and_then(|g| g.as_array()) {
                for group in groups {
                    if let Some(controls) = group.get("controls").and_then(|c| c.as_array()) {
                        for control in controls {
                            line_number += 1;
                            let record = self.parse_control(control, line_number);
                            records.push(record);
                        }
                    }
                }
            }

            // Also check for top-level controls
            if let Some(controls) = catalog.get("controls").and_then(|c| c.as_array()) {
                for control in controls {
                    line_number += 1;
                    let record = self.parse_control(control, line_number);
                    records.push(record);
                }
            }
        }

        Ok(records)
    }

    fn parse_control(&self, control: &serde_json::Value, line_number: usize) -> ImportRecord {
        let mut data = HashMap::new();

        if let Some(id) = control.get("id").and_then(|v| v.as_str()) {
            data.insert("control_id".to_string(), serde_json::Value::String(id.to_string()));
        }

        if let Some(title) = control.get("title").and_then(|v| v.as_str()) {
            data.insert("title".to_string(), serde_json::Value::String(title.to_string()));
        }

        if let Some(class) = control.get("class").and_then(|v| v.as_str()) {
            data.insert("class".to_string(), serde_json::Value::String(class.to_string()));
        }

        // Extract prose from parts
        if let Some(parts) = control.get("parts").and_then(|p| p.as_array()) {
            for part in parts {
                if let Some(prose) = part.get("prose").and_then(|p| p.as_str()) {
                    data.insert("description".to_string(), serde_json::Value::String(prose.to_string()));
                    break;
                }
            }
        }

        // Extract parameters
        if let Some(params) = control.get("params").and_then(|p| p.as_array()) {
            data.insert("parameters".to_string(), serde_json::Value::Array(params.clone()));
        }

        // Apply defaults
        for (key, default) in &self.config.defaults {
            data.entry(key.clone()).or_insert_with(|| default.clone());
        }

        ImportRecord::new(line_number, data)
    }

    /// Parse OSCAL component definition
    pub fn parse_component<R: Read>(&self, reader: R) -> Result<Vec<ImportRecord>, ImportError> {
        let doc: serde_json::Value = serde_json::from_reader(reader)
            .map_err(|e| ImportError {
                line_number: None,
                field: None,
                message: format!("Invalid OSCAL JSON: {}", e),
                severity: ErrorSeverity::Fatal,
            })?;

        let mut records = Vec::new();
        let mut line_number = 0;

        if let Some(comp_def) = doc.get("component-definition") {
            if let Some(components) = comp_def.get("components").and_then(|c| c.as_array()) {
                for component in components {
                    line_number += 1;

                    let mut data = HashMap::new();

                    if let Some(uuid) = component.get("uuid").and_then(|v| v.as_str()) {
                        data.insert("uuid".to_string(), serde_json::Value::String(uuid.to_string()));
                    }

                    if let Some(title) = component.get("title").and_then(|v| v.as_str()) {
                        data.insert("title".to_string(), serde_json::Value::String(title.to_string()));
                    }

                    if let Some(ctype) = component.get("type").and_then(|v| v.as_str()) {
                        data.insert("type".to_string(), serde_json::Value::String(ctype.to_string()));
                    }

                    if let Some(desc) = component.get("description").and_then(|v| v.as_str()) {
                        data.insert("description".to_string(), serde_json::Value::String(desc.to_string()));
                    }

                    // Apply defaults
                    for (key, default) in &self.config.defaults {
                        data.entry(key.clone()).or_insert_with(|| default.clone());
                    }

                    records.push(ImportRecord::new(line_number, data));
                }
            }
        }

        Ok(records)
    }
}

/// Validation function type
pub type ValidatorFn = Box<dyn Fn(&ImportRecord) -> Vec<String> + Send + Sync>;

/// Record validator
pub struct RecordValidator {
    validators: Vec<ValidatorFn>,
}

impl Default for RecordValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordValidator {
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    /// Add a required field validator
    pub fn require_field(mut self, field: &'static str) -> Self {
        self.validators.push(Box::new(move |record| {
            if record.data.contains_key(field) {
                vec![]
            } else {
                vec![format!("Missing required field: {}", field)]
            }
        }));
        self
    }

    /// Add a custom validator
    pub fn add_validator<F>(mut self, validator: F) -> Self
    where
        F: Fn(&ImportRecord) -> Vec<String> + Send + Sync + 'static,
    {
        self.validators.push(Box::new(validator));
        self
    }

    /// Validate a record
    pub fn validate(&self, record: &mut ImportRecord) {
        for validator in &self.validators {
            let errors = validator(record);
            for error in errors {
                record.errors.push(error);
                record.valid = false;
            }
        }
    }

    /// Validate all records
    pub fn validate_all(&self, records: &mut [ImportRecord]) {
        for record in records {
            self.validate(record);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_csv_import() {
        let csv_data = "id,name,value\n1,foo,100\n2,bar,200";
        let importer = CsvImporter::default();
        let records = importer.parse(Cursor::new(csv_data)).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get_string("name"), Some("foo".to_string()));
        // Numbers are parsed as JSON numbers, so use get_i64
        assert_eq!(records[1].get_i64("value"), Some(200));
    }

    #[test]
    fn test_csv_with_mapping() {
        let csv_data = "ID,Name,Amount\n1,test,50";
        let mut mapping = HashMap::new();
        mapping.insert("ID".to_string(), "id".to_string());
        mapping.insert("Name".to_string(), "name".to_string());
        mapping.insert("Amount".to_string(), "value".to_string());

        let config = ImportConfig {
            field_mapping: Some(mapping),
            ..Default::default()
        };
        let importer = CsvImporter::new(config);
        let records = importer.parse(Cursor::new(csv_data)).unwrap();

        assert_eq!(records[0].get_string("name"), Some("test".to_string()));
        // Numbers are parsed as JSON numbers, so use get_i64
        assert_eq!(records[0].get_i64("value"), Some(50));
    }

    #[test]
    fn test_json_import() {
        let json_data = r#"[{"id": 1, "name": "foo"}, {"id": 2, "name": "bar"}]"#;
        let importer = JsonImporter::default();
        let records = importer.parse_json(Cursor::new(json_data)).unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].get_i64("id"), Some(1));
        assert_eq!(records[1].get_string("name"), Some("bar".to_string()));
    }

    #[test]
    fn test_jsonl_import() {
        let jsonl_data = "{\"id\": 1, \"name\": \"foo\"}\n{\"id\": 2, \"name\": \"bar\"}\n";
        let importer = JsonImporter::default();
        let records = importer.parse_jsonl(Cursor::new(jsonl_data)).unwrap();

        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.valid));
    }

    #[test]
    fn test_oscal_catalog_import() {
        let oscal_data = r#"{
            "catalog": {
                "groups": [{
                    "controls": [{
                        "id": "AC-1",
                        "title": "Access Control Policy",
                        "parts": [{"prose": "The organization develops..."}]
                    }]
                }]
            }
        }"#;

        let importer = OscalImporter::default();
        let records = importer.parse_catalog(Cursor::new(oscal_data)).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].get_string("control_id"), Some("AC-1".to_string()));
        assert_eq!(records[0].get_string("title"), Some("Access Control Policy".to_string()));
    }

    #[test]
    fn test_record_validation() {
        let mut record = ImportRecord::new(1, HashMap::new());
        record.data.insert("name".to_string(), serde_json::Value::String("test".to_string()));

        let validator = RecordValidator::new()
            .require_field("name")
            .require_field("id");

        validator.validate(&mut record);

        assert!(!record.valid);
        assert!(record.errors.iter().any(|e| e.contains("id")));
    }

    #[test]
    fn test_import_format_detection() {
        assert_eq!(
            ImportFormat::from_extension(Path::new("data.csv")),
            Some(ImportFormat::Csv)
        );
        assert_eq!(
            ImportFormat::from_extension(Path::new("data.json")),
            Some(ImportFormat::Json)
        );
        assert_eq!(
            ImportFormat::from_extension(Path::new("data.jsonl")),
            Some(ImportFormat::JsonL)
        );
    }

    #[test]
    fn test_oscal_format_detection() {
        assert_eq!(
            ImportFormat::detect_oscal(r#"{"catalog": {}}"#),
            Some(ImportFormat::OscalCatalog)
        );
        assert_eq!(
            ImportFormat::detect_oscal(r#"{"system-security-plan": {}}"#),
            Some(ImportFormat::OscalSsp)
        );
    }
}
