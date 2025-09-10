use crate::openai;
use csv::{Reader, ReaderBuilder, WriterBuilder};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    fs::{self, File},
    io::{ErrorKind, Read},
    path::Path,
};

const CONFTOOL_DATA_DIR_PATH: &str = "../data/conftool";
const OUTPUT_DATA_DIR_PATH: &str = "../data/output";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Reviewer {
    paper_id: usize,
    raw_name: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    institution: Option<String>,
    email: Option<String>,
}

/// Parses the CSV with mandatory reviewers.
pub fn parse_mandatory_reviewers(overwrite: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure the data dirs exist
    let conftool_data_dir = Path::new(CONFTOOL_DATA_DIR_PATH);
    let output_data_dir = Path::new(OUTPUT_DATA_DIR_PATH);

    fs::create_dir_all(conftool_data_dir)?;
    fs::create_dir_all(output_data_dir)?;

    // Check whether the mandatory reviewers file exists
    let reviewers_source_csv_path = conftool_data_dir.join("mandatory_reviewers_raw.csv");

    if !reviewers_source_csv_path.exists() {
        return Err("Mandatory reviewers CSV file not found.".into());
    }

    println!("+++ MANDATORY REVIEWER PARSING STARTED +++");

    // Load the CSV file and parse the raw reviewer info
    let mut raw_reviewers = load_reviewers_from_csv(&reviewers_source_csv_path)?;

    println!("Number of raw entries: {:?}", raw_reviewers.len());

    // Remove the duplicates
    let mut unique_names = HashSet::new();
    raw_reviewers.retain(|reviewer| unique_names.insert(reviewer.raw_name.clone()));

    println!("Number of unique raw entries: {:?}", raw_reviewers.len());

    // Parse the reviewer info using LLMs
    let successful_reviewers_file_path = output_data_dir.join("reviewers_parsed.csv");
    let failed_reviewers_file_path = output_data_dir.join("reviewers_failed.csv");

    if overwrite
        || !Path::exists(&successful_reviewers_file_path)
        || !Path::exists(&failed_reviewers_file_path)
    {
        let (successful_reviewers, failed_reviewers) = llm_parse(&raw_reviewers)?;

        println!("Successful reviewer parses: {}", successful_reviewers.len());
        println!("Failed reviewer parses: {}", failed_reviewers.len());

        // Export the CSV files
        export_reviewers_to_csv(&successful_reviewers, &successful_reviewers_file_path)?;
        export_reviewers_to_csv(&failed_reviewers, &failed_reviewers_file_path)?;
    } else {
        println!("LLM parsing completed previously, skipping.");
    }

    // Load the manually processed reviewer file (after the prev step)
    let reviewers_all_path = output_data_dir.join("reviewers_all.csv");
    let reviewers_all = load_reviewers_from_csv(&reviewers_all_path)?;

    println!(
        "Total valid reviewer entries with emails: {}",
        reviewers_all.len()
    );

    // Prune reviewers that are already TPC members
    let reviewers_final = prune_existing_tpc_members(reviewers_all)?;

    println!(
        "Total new reviewers that are not existing TPC members: {}",
        reviewers_final.len()
    );

    // Export the final mandatory reviewers list
    let reviewers_final_path = output_data_dir.join("reviewers_final.csv");
    export_reviewers_to_csv(&reviewers_final, &reviewers_final_path)?;

    println!("+++ MANDATORY REVIEWER PARSING COMPLETED +++");

    Ok(())
}

/// Return the index of a CSV header
fn csv_header_index(
    csv_reader: &mut Reader<File>,
    header: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let csv_headers = csv_reader.headers()?.clone();
    let header_idx = csv_headers
        .iter()
        .position(|h| h == header)
        .ok_or(format!("Column {header} not found in submissions.csv."))?;

    Ok(header_idx)
}

/// Exports a list of reviewers into CSV with the given file name
fn export_reviewers_to_csv(
    reviewers: &[Reviewer],
    csv_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let csv_file = File::create(csv_path)?;

    let mut csv_writer = WriterBuilder::new().delimiter(b';').from_writer(csv_file);

    for reviewer in reviewers {
        csv_writer.serialize(reviewer)?;
    }

    csv_writer.flush()?;

    Ok(())
}

/// Using the raw list of reviewers,
fn llm_parse(
    raw_reviewers: &[Reviewer],
) -> Result<(Vec<Reviewer>, Vec<Reviewer>), Box<dyn std::error::Error>> {
    // Load the prompts and prompt templates
    const PROMPT_FILE_NAME: &str = "mandatory_reviewer_parse.md";

    let sys_prompt_path = Path::new(openai::SYSTEM_PROMPTS_DIR_PATH).join(PROMPT_FILE_NAME);
    let user_prompt_template_path = Path::new(openai::USER_PROMPTS_DIR_PATH).join(PROMPT_FILE_NAME);

    let sys_prompt = fs::read_to_string(sys_prompt_path)?;
    let user_prompt_template = fs::read_to_string(user_prompt_template_path)?;

    // Load the submissions data into a HashMap with paper ID keys and the CSV row as values
    let submissions_data = load_submissions_data()?;

    // Process reviewers in batches
    const BATCH_SIZE: usize = 5;
    let n_batches = raw_reviewers.len().div_ceil(BATCH_SIZE);
    let mut processed_reviewers: Vec<Reviewer> = Vec::new();
    let mut failed_reviewers: Vec<Reviewer> = Vec::new();

    for (i, batch) in raw_reviewers.chunks(BATCH_SIZE).enumerate() {
        // Establish the batch submissions data
        let batch_paper_ids: Vec<usize> = batch.iter().map(|reviewer| reviewer.paper_id).collect();
        let batch_submissions_data = batch_paper_ids
            .iter()
            .filter_map(|paper_id| submissions_data.get(paper_id))
            .cloned()
            .collect::<Vec<String>>()
            .join("\n");

        // Establish the batch raw reviewer data
        let batch_reviewer_raw_data = serde_json::to_string(batch)?;

        // Assemble the user prompt
        let mut user_prompt = user_prompt_template.clone();
        let placeholder_replacements = [
            ("<REVIEWER_RAW_DATA>", &batch_reviewer_raw_data),
            ("<SUBMISSION_DETAILS>", &batch_submissions_data),
        ];

        for (placeholder, value) in placeholder_replacements {
            user_prompt = user_prompt.replace(placeholder, value);
        }

        const MAX_LLM_ATTEMPTS: usize = 3;
        let mut n_attempts: usize = 0;
        let mut batch_reviewers: Option<Vec<Reviewer>> = None;

        while let None = batch_reviewers
            && n_attempts < MAX_LLM_ATTEMPTS
        {
            // Call the LLM
            let response = match openai::chat_response(&sys_prompt, &user_prompt)? {
                Some(r) => r,
                None => {
                    return Err(format!(
                        "OpenAI API returned no response for reviewer batch {}.",
                        i + 1
                    )
                    .into());
                }
            };
            // Record the conversation
            let conversation_id = format!("mandatory_reviewer_parse-{}-{}", i, n_attempts);
            write_llm_conversation_to_file(conversation_id, &sys_prompt, &user_prompt, &response)?;

            // Parse the batch reviewers
            batch_reviewers = serde_json::from_str(&response).ok();

            n_attempts += 1;
        }

        // If the batch reviewers are still None after all the attempts, we throw an error
        if batch_reviewers.is_none() {
            failed_reviewers.extend(batch.iter().cloned());
            println!(
                "- Reviewer batch {}/{}: failed serialization. {} reviewers parsed successfully, {} failures.",
                i + 1,
                n_batches,
                processed_reviewers.len(),
                failed_reviewers.len()
            );
            continue;
        }

        // Iterate over the reviewers and base success on whether the e-mail was parsed successfully
        for reviewer in batch_reviewers.unwrap() {
            if reviewer.email.is_some() {
                processed_reviewers.push(reviewer);
            } else {
                failed_reviewers.push(reviewer)
            }
        }

        println!(
            "- Reviewer batch {}/{} processed. {} reviewers parsed successfully, {} failures.",
            i + 1,
            n_batches,
            processed_reviewers.len(),
            failed_reviewers.len()
        );
    }

    Ok((processed_reviewers, failed_reviewers))
}

/// Load Reviewer objects from a CSV
fn load_reviewers_from_csv(csv_path: &Path) -> Result<Vec<Reviewer>, Box<dyn std::error::Error>> {
    let csv_file = File::open(csv_path)?;
    let mut reader = ReaderBuilder::new().delimiter(b';').from_reader(csv_file);
    let mut reviewers: Vec<Reviewer> = Vec::new();

    for result in reader.deserialize() {
        let reviewer = result?;
        reviewers.push(reviewer);
    }

    Ok(reviewers)
}

/// Loads the paper submission data from CSV and stores them in a HashMap with paper IDs as keys and
/// submission records (CSV rows) as values.
fn load_submissions_data() -> Result<HashMap<usize, String>, Box<dyn std::error::Error>> {
    let data_dir_path = Path::new(CONFTOOL_DATA_DIR_PATH);
    let submissions_data_path = data_dir_path.join("submissions.csv");
    let submissions_data_file = File::open(submissions_data_path)?;
    let mut csv_reader = ReaderBuilder::new()
        .delimiter(b';')
        .from_reader(submissions_data_file);
    let paper_id_idx = csv_header_index(&mut csv_reader, "paperID")?;

    let mut submissions_data: HashMap<usize, String> = HashMap::new();

    for result in csv_reader.records() {
        let record = result?;
        if let Some(paper_id) = record.get(paper_id_idx) {
            let row_string = record.iter().collect::<Vec<_>>().join(",");
            submissions_data.insert(paper_id.parse()?, row_string);
        }
    }

    Ok(submissions_data)
}

/// Given the list of reviewers, prunes those that are already TPC members
fn prune_existing_tpc_members(
    mut reviewers: Vec<Reviewer>,
) -> Result<Vec<Reviewer>, Box<dyn std::error::Error>> {
    // Load the emails of the TPC members from CSV into HashSet
    let tpc_members_csv_path = Path::new(CONFTOOL_DATA_DIR_PATH).join("tpc_members.csv");
    let tpc_members_csv_file = File::open(tpc_members_csv_path)?;
    let mut csv_reader = ReaderBuilder::new()
        .delimiter(b';')
        .from_reader(tpc_members_csv_file);

    let email_idx = csv_header_index(&mut csv_reader, "email")?;

    let tpc_emails: HashSet<String> = csv_reader
        .records()
        .filter_map(|result| result.ok())
        .filter_map(|record| record.get(email_idx).map(|s| s.to_string()))
        .collect();

    // Filter out those reviewers who are in the TPC email list or that don't have an email
    reviewers.retain(|reviewer| match &reviewer.email {
        Some(email) => !tpc_emails.contains(email),
        None => false,
    });

    Ok(reviewers)
}

fn write_llm_conversation_to_file(
    conversation_id: String,
    sys_prompt: &String,
    user_prompt: &String,
    response: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let llm_response_dir_path = Path::new(OUTPUT_DATA_DIR_PATH).join("llm_responses");
    fs::create_dir_all(&llm_response_dir_path)?;

    // match fs::create_dir_all(&llm_response_dir_path) {
    //     Ok(()) => println!("Created directory: {:?}", &llm_response_dir_path),
    //     Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
    //     Err(e) => return Err(e.into()),
    // }

    let output_file_name = format!("{}.txt", conversation_id);
    let output_file_path = llm_response_dir_path.join(output_file_name);

    let content = format!(
        "SYSTEM PROMPT:\n{}\n\nUSER PROMPT:\n{}\n\nRESPONSE:\n{}",
        sys_prompt, user_prompt, response,
    );

    fs::write(output_file_path, &content)?;

    Ok(())
}
