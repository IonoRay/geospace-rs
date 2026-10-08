//! Sequential JSONL exercise through the production CLI record processor.
//!
//! The three lines use only IGRF, so they neither open an index store nor use
//! a user data home.  The middle invalid latitude must not prevent line three.

use std::io::BufReader;

use ionoray_geospace::cli::{ExecuteStatus, process};

const RECORDS: &[u8] = br#"{"id":"a","model":"igrf14","mode":"direct","at":"2020-07-01T12:00:00Z","latitude_deg":30,"longitude_deg":120,"altitude_km":300}
{"id":"bad","model":"igrf14","mode":"direct","at":"2020-07-01T12:00:00Z","latitude_deg":91,"longitude_deg":120,"altitude_km":300}
{"id":"b","model":"igrf14","mode":"auto","data_policy":"offline","at":"2020-07-01T12:00:00Z","latitude_deg":30,"longitude_deg":120,"altitude_km":300}
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Breakpoint 1: the production JSONL parser receives three physical lines.
    let mut output = Vec::new();
    let status = process(None, BufReader::new(RECORDS), &mut output, true).await?;
    // Breakpoint 2: this must be RecordFailures, while all three records exist.
    assert_eq!(status, ExecuteStatus::RecordFailures);
    let output = String::from_utf8(output)?;
    // Breakpoint 3: inspect JSONL `line`, `id`, and `status` before printing.
    print!("{output}");
    Ok(())
}
