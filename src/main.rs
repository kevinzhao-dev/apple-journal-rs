use anyhow::Result;
use clap::Parser;
use journal_rs::{cli::Cli, execute, presentation};
use std::io::Write;
fn run(cli: Cli) -> Result<()> {
    if matches!(cli.command, journal_rs::cli::Command::Mcp) {
        return tokio::runtime::Runtime::new()?
            .block_on(journal_rs::mcp::serve(cli.database_path()?));
    }
    let response = execute(&cli.database_path()?, &cli.command)?;
    let output = presentation::format(&response, &cli.command)?;
    std::io::stderr().lock().write_all(&output.stderr)?;
    std::io::stdout().lock().write_all(&output.stdout)?;
    Ok(())
}
fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() { 1 } else { 0 };
            let _ = error.print();
            std::process::exit(code)
        }
    };
    if let Err(error) = run(cli) {
        eprintln!("journal-rs: {error:#}");
        std::process::exit(1);
    }
}
