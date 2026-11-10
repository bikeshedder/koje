//! Interactive yes/no questions on the terminal.

use std::io::{BufRead as _, Write as _};

use anyhow::{Context as _, Result};

/// Ask to choose one of `choices` (`(value, title)`) by number; `None` at
/// the end of input or for an answer that is none of them.
pub fn choose(question: &str, choices: &[(String, String)]) -> Result<Option<String>> {
    println!("{question}");
    for (i, (value, title)) in choices.iter().enumerate() {
        println!("  {}) {title}  ({value})", i + 1);
    }
    print!("number: ");
    std::io::stdout().flush().context("writing to stdout")?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading from stdin")?;
    Ok(line
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|n| choices.get(n.wrapping_sub(1)))
        .map(|(value, _)| value.clone()))
}

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
