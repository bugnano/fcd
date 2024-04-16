const ID_CHARS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

#[derive(Debug, Clone, Copy)]
enum IdentifierState {
    None,
    Simple,
    Braced,
}

pub fn substitute<T: IntoIterator<Item = (U, V)>, U: AsRef<str>, V: AsRef<str>>(
    template: &str,
    mapping: T,
    delimiter: char,
) -> String {
    let mapping: Vec<(String, String)> = mapping
        .into_iter()
        .map(|(k, v)| (String::from(k.as_ref()), String::from(v.as_ref())))
        .collect();

    let mut result = String::new();
    let mut tmp = String::new();
    let mut identifier_state = IdentifierState::None;

    for c in template.chars() {
        match (c, &identifier_state) {
            (c, IdentifierState::None) if c == delimiter => {
                identifier_state = IdentifierState::Simple;
            }
            (c, IdentifierState::None) => {
                result.push(c);
            }
            (c, IdentifierState::Simple) if c == delimiter && tmp.is_empty() => {
                result.push(delimiter);
                identifier_state = IdentifierState::None;
            }
            ('{', IdentifierState::Simple) if tmp.is_empty() => {
                identifier_state = IdentifierState::Braced;
            }
            (c, _) if ID_CHARS.contains(c) => {
                tmp.push(c);
            }
            (c, _) => {
                let mut replaced = false;

                if matches!(identifier_state, IdentifierState::Simple) || c == '}' {
                    for (identifier, replacement) in &mapping {
                        if &tmp == identifier {
                            result.push_str(replacement);
                            replaced = true;
                            break;
                        }
                    }
                }

                match (c, &identifier_state) {
                    (c, IdentifierState::Simple) if c == delimiter => {
                        if !replaced {
                            result.push_str(&format!("{}{}", delimiter, tmp));
                        }
                    }
                    (c, IdentifierState::Braced) if c == delimiter => {
                        if !replaced {
                            result.push_str(&format!("{}{{{}", delimiter, tmp));
                        }

                        identifier_state = IdentifierState::Simple;
                    }
                    (c, IdentifierState::Simple) => {
                        match replaced {
                            true => result.push(c),
                            false => result.push_str(&format!("{}{}{}", delimiter, tmp, c)),
                        }

                        identifier_state = IdentifierState::None;
                    }
                    (c, IdentifierState::Braced) => {
                        match replaced {
                            true => {
                                if c != '}' {
                                    result.push(c);
                                }
                            }
                            false => result.push_str(&format!("{}{{{}{}", delimiter, tmp, c)),
                        }

                        identifier_state = IdentifierState::None;
                    }
                    _ => unreachable!(),
                }

                tmp.clear();
            }
        }
    }

    if !matches!(identifier_state, IdentifierState::None) {
        let mut replaced = false;

        if let IdentifierState::Simple = identifier_state {
            for (identifier, replacement) in &mapping {
                if &tmp == identifier {
                    result.push_str(replacement);
                    replaced = true;
                    break;
                }
            }
        }

        match &identifier_state {
            IdentifierState::Simple => {
                if !replaced {
                    result.push_str(&format!("{}{}", delimiter, tmp));
                }
            }
            IdentifierState::Braced => {
                if !replaced {
                    result.push_str(&format!("{}{{{}", delimiter, tmp));
                }
            }
            _ => unreachable!(),
        }
    }

    result
}
