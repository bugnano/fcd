use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};

const BOOKMARK_KEYS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

#[derive(Debug)]
pub struct Bookmarks {
    file: Option<PathBuf>,
    data: Vec<(char, PathBuf)>,
}

impl Bookmarks {
    pub fn new(file: Option<&Path>) -> Bookmarks {
        let data = match file.map(File::open) {
            Some(Ok(f)) => {
                let lines: Vec<String> = BufReader::new(f).lines().map_while(Result::ok).collect();

                lines
                    .iter()
                    .filter_map(|line| line.split_once(':'))
                    .filter(|(k, _v)| k.len() == 1 && BOOKMARK_KEYS.contains(k))
                    .map(|(k, v)| (k.chars().next().unwrap(), PathBuf::from(v)))
                    .collect()
            }
            _ => Vec::new(),
        };

        Bookmarks {
            file: file.map(PathBuf::from),
            data,
        }
    }

    pub fn get(&self, key: char) -> Option<PathBuf> {
        self.data.iter().find_map(|(k, v)| match *k == key {
            true => Some(v.clone()),
            false => None,
        })
    }

    pub fn insert(&mut self, key: char, item: &Path) {
        if !BOOKMARK_KEYS.contains(key) {
            return;
        }

        let should_update = match self.get(key) {
            Some(existing_value) => match existing_value == item {
                true => false,
                false => {
                    let i = self.data.iter().position(|(k, _v)| *k == key).unwrap();

                    self.data[i] = (key, PathBuf::from(item));

                    true
                }
            },
            None => {
                self.data.push((key, PathBuf::from(item)));

                true
            }
        };

        if should_update {
            self.data.sort();
            self.update_file();
        }
    }

    pub fn remove(&mut self, key: char) {
        let index = self.data.iter().position(|(k, _v)| *k == key);

        if let Some(i) = index {
            self.data.remove(i);
            self.update_file();
        }
    }

    fn update_file(&mut self) {
        if let Some(file) = &self.file {
            let af = AtomicFile::new(file, AllowOverwrite);
            let _ = af.write(|f| {
                let mut result = Ok(());

                for (k, v) in &self.data {
                    result = writeln!(f, "{}:{}", k, v.to_string_lossy());
                }

                result
            });
        }
    }
}
