//! Terminal and stdin password input.

use crate::Result;
use anyhow::{bail, Context};
use std::io::BufRead;
use zeroize::Zeroizing;

pub(crate) fn prompt(confirm: bool) -> Result<String> {
    prompt_with(confirm, |message| {
        rpassword::prompt_password(message).context(
            "Cannot read password securely from terminal; use --password-stdin for scripts",
        )
    })
}

fn prompt_with(confirm: bool, mut read: impl FnMut(&str) -> Result<String>) -> Result<String> {
    let password = Zeroizing::new(read("Password: ")?);
    if password.is_empty() {
        bail!("Password cannot be empty");
    }
    if confirm {
        let confirmation = Zeroizing::new(read("Confirm password: ")?);
        if *password != *confirmation {
            bail!("Passwords do not match");
        }
    }
    Ok(password.to_string())
}

pub(crate) fn read_line(reader: &mut impl BufRead) -> Result<String> {
    let mut password = String::new();
    reader.read_line(&mut password)?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.is_empty() {
        bail!("Password cannot be empty");
    }
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_preserves_password_except_line_ending() {
        for input in ["  páßword  \n", "  páßword  \r\n", "  páßword  "] {
            assert_eq!(read_line(&mut input.as_bytes()).unwrap(), "  páßword  ");
        }
        assert_eq!(read_line(&mut &b"first\nsecond\n"[..]).unwrap(), "first");
        for input in ["", "\n", "\r\n"] {
            assert!(read_line(&mut input.as_bytes()).is_err());
        }
    }

    #[test]
    fn creating_requires_matching_nonempty_passwords() {
        let mut answers = ["secret", "secret"].into_iter();
        assert_eq!(
            prompt_with(true, |_| Ok(answers.next().unwrap().into())).unwrap(),
            "secret"
        );
        assert!(answers.next().is_none());

        let mut answers = ["secret", "different"].into_iter();
        let error = prompt_with(true, |_| Ok(answers.next().unwrap().into())).unwrap_err();
        assert_eq!(error.to_string(), "Passwords do not match");
        assert!(prompt_with(true, |_| Ok(String::new())).is_err());
    }

    #[test]
    fn extracting_prompts_once_and_input_errors_propagate() {
        let mut answers = ["secret"].into_iter();
        assert_eq!(
            prompt_with(false, |_| Ok(answers.next().unwrap().into())).unwrap(),
            "secret"
        );
        assert!(prompt_with(false, |_| bail!("terminal unavailable")).is_err());
    }
}
