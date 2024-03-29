use regex::Regex;

pub fn substitute<T: IntoIterator<Item = (U, V)>, U: AsRef<str>, V: AsRef<str>>(
    template: &str,
    mapping: T,
    delimiter: &str,
) -> String {
    let mut result = String::from(template);
    let escaped_delimiter = regex::escape(delimiter);

    for (identifier, replacement) in mapping {
        let escaped_identifier = regex::escape(identifier.as_ref());

        // Substitute $identifier
        result = Regex::new(&format!(r"{}{}\b", escaped_delimiter, escaped_identifier))
            .unwrap()
            .replace_all(&result, replacement.as_ref())
            .to_string();

        // Substitute ${identifier}
        result = Regex::new(&format!(
            "{}[{{]{}[}}]",
            escaped_delimiter, escaped_identifier
        ))
        .unwrap()
        .replace_all(&result, replacement.as_ref())
        .to_string();
    }

    // Substitute $$
    result = Regex::new(&format!("{}{}", escaped_delimiter, escaped_delimiter))
        .unwrap()
        .replace_all(&result, delimiter)
        .to_string();

    result
}
