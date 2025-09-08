use std::fs;
use std::path::Path;

const DATA_DIR: &str = "../conftool_data";

pub fn parse_mandatory_reviewers() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = Path::new(DATA_DIR);

    if !data_dir.exists() {
        return Err("conftool_data directory not found.".into());
    }

    let mandatory_reviewers_source_csv = data_dir.join("mandatory_reviewers.csv");

    if !mandatory_reviewers_source_csv.exists() {
        return Err("Mandatory reviewers CSV file not found.".into());
    }

    println!("Parsing mandatory reviewers from conftool_data...");

    // Load the CSV file.

    Ok(())
}
