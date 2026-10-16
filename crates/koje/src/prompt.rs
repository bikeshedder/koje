//! Interactive yes/no questions on the terminal.

use std::io::{BufRead as _, Write as _};

use anyhow::{Context as _, Result};

/// Ask `question` and read one line from stdin. Empty input means `default`;
/// end of input (e.g. no terminal) means no.
pub fn confirm(question: &str, default: bool) -> Result<bool> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    print!("{question} {hint} ");
    std::io::stdout().flush().context("writing to stdout")?;
    let mut line = String::new();
    let read = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading from stdin")?;
    if read == 0 {
        println!();
        return Ok(false);
    }
    Ok(match line.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    })
}
