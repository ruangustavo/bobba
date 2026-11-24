#[cfg(any(target_os = "macos", test))]
pub fn posix<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    words
        .into_iter()
        .map(|word| format!("'{}'", word.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(windows, test))]
pub fn windows<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    words
        .into_iter()
        .map(windows_word)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(windows, test))]
fn windows_word(word: &str) -> String {
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in word.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(c);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_words_survive_spaces_and_quotes() {
        assert_eq!(
            posix(["/Applications/Bob ba.app/bobba", "it's"]),
            r"'/Applications/Bob ba.app/bobba' 'it'\''s'"
        );
    }

    #[test]
    fn windows_words_survive_spaces_quotes_and_trailing_backslashes() {
        assert_eq!(
            windows([r"C:\Users\Ana Lu\Temp\bobba-hosts-1", r#"a"b"#, r"dir\"]),
            r#""C:\Users\Ana Lu\Temp\bobba-hosts-1" "a\"b" "dir\\""#
        );
    }
}
