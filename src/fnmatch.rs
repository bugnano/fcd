// The fnmatch.translate function adapted from Python 3.6
pub fn translate(pattern: &str) -> String {
    let pat: Vec<char> = pattern.chars().collect();
    let mut res = Vec::new();

    let mut i = 0;
    let n = pat.len();

    while i < n {
        match pat[i] {
            '*' => {
                res.push('.');
                res.push('*');
            }
            '?' => res.push('.'),
            '[' => {
                let mut j = i + 1;

                if (j < n) && (pat[j] == '!') {
                    j += 1;
                }

                if (j < n) && (pat[j] == ']') {
                    j += 1;
                }

                while (j < n) && (pat[j] != ']') {
                    j += 1;
                }

                if j >= n {
                    res.push('\\');
                    res.push('[');
                } else {
                    let mut stuff: Vec<char> = pat[(i + 1)..j]
                        .iter()
                        .collect::<String>()
                        .replace('\\', "\\\\")
                        .chars()
                        .collect();

                    i = j;

                    res.push('[');

                    match stuff[0] {
                        '!' => stuff[0] = '^',
                        '^' => res.push('\\'),
                        _ => (),
                    }

                    res.extend(&stuff);
                    res.push(']');
                }
            }
            c => res.extend(regex::escape(&String::from(c)).chars()),
        }

        i += 1;
    }

    format!("(?s:{})\\z", res.iter().collect::<String>())
}
