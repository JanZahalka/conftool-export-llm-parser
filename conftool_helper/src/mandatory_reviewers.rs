use crate::openai;
use csv::Reader;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::Path,
};

const DATA_DIR_PATH: &str = "../conftool_data";

#[derive(Debug, Deserialize, Serialize)]
struct Reviewer {
    paper_id: usize,
    raw_name: String,
    #[serde(skip)]
    first_name: Option<String>,
    #[serde(skip)]
    last_name: Option<String>,
    #[serde(skip)]
    institution: Option<String>,
    #[serde(skip)]
    email: Option<String>,
}

/// Parses the CSV with mandatory reviewers.
pub fn parse_mandatory_reviewers() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the data dir exists
    let data_dir = Path::new(DATA_DIR_PATH);

    match fs::create_dir_all(data_dir) {
        Ok(()) => println!("Created directory: {DATA_DIR_PATH}"),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }

    // Check whether the mandatory reviewers file exists
    let mandatory_reviewers_source_csv = data_dir.join("mandatory_reviewers.csv");

    if !mandatory_reviewers_source_csv.exists() {
        return Err("Mandatory reviewers CSV file not found.".into());
    }

    println!("+++ MANDATORY REVIEWER PARSING STARTED +++");

    // Load the CSV file and parse the raw reviewer info
    let mut reader = Reader::from_path(&mandatory_reviewers_source_csv)?;
    let mut reviewers: Vec<Reviewer> = Vec::new();

    for result in reader.deserialize() {
        let reviewer = result?;
        reviewers.push(reviewer);
    }

    println!("Number of raw entries: {:?}", reviewers.len());

    // Remove the duplicates
    let mut unique_names = HashSet::new();
    reviewers.retain(|reviewer| unique_names.insert(reviewer.raw_name.clone()));

    println!("Number of unique raw entries: {:?}", reviewers.len());

    _ = llm_parse(reviewers)?;

    Ok(())
}

/// Using the raw list of reviewers,
fn llm_parse(mut reviewers: Vec<Reviewer>) -> Result<Vec<Reviewer>, Box<dyn std::error::Error>> {
    // Load the prompt template, user data, and submissions data
    let sys_prompt_template_path =
        Path::new(openai::SYSTEM_PROMPTS_DIR_PATH).join("mandatory_reviewer_parse.md");

    let data_dir_path = Path::new(DATA_DIR_PATH);
    let user_data_path = data_dir_path.join("users_all.csv");
    let submissions_data_path = data_dir_path.join("submissions.csv");

    let sys_prompt_template = fs::read_to_string(sys_prompt_template_path)?;
    let user_data = fs::read_to_string(user_data_path)?;
    let submissions_data = fs::read_to_string(submissions_data_path)?;

    // Assemble the system prompt from the template and the data
    let mut sys_prompt = sys_prompt_template;
    let placeholder_replacements = [
        ("<USER_DATA>", &user_data),
        ("<SUBMISSION_DETAILS>", &submissions_data),
    ];

    for (placeholder, value) in placeholder_replacements {
        sys_prompt = sys_prompt.replace(placeholder, value);
    }

    // Process reviewers in batches
    const BATCH_SIZE: usize = 5;

    for batch in reviewers.chunks(BATCH_SIZE) {
        let user_prompt = serde_json::to_string(batch)?;

        // Call the LLM
        let response = openai::chat_response(&sys_prompt, &user_prompt)?;

        if let Some(response_text) = response {
            println!("{response_text}");
        }

        break;
    }

    Ok(reviewers)
}
