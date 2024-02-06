use libc::{
    S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFREG, S_IFSOCK, S_IRGRP, S_IROTH, S_IRUSR,
    S_ISGID, S_ISUID, S_ISVTX, S_IWGRP, S_IWOTH, S_IWUSR, S_IXGRP, S_IXOTH, S_IXUSR,
};

const FILEMODE_TABLE: &[&[(u32, &str)]] = &[
    &[
        (S_IFLNK, "l"),
        (S_IFSOCK, "s"), // Must appear before IFREG and IFDIR as IFSOCK == IFREG | IFDIR
        (S_IFREG, "-"),
        (S_IFBLK, "b"),
        (S_IFDIR, "d"),
        (S_IFCHR, "c"),
        (S_IFIFO, "p"),
    ],
    &[(S_IRUSR, "r")],
    &[(S_IWUSR, "w")],
    &[(S_IXUSR | S_ISUID, "s"), (S_ISUID, "S"), (S_IXUSR, "x")],
    &[(S_IRGRP, "r")],
    &[(S_IWGRP, "w")],
    &[(S_IXGRP | S_ISGID, "s"), (S_ISGID, "S"), (S_IXGRP, "x")],
    &[(S_IROTH, "r")],
    &[(S_IWOTH, "w")],
    &[(S_IXOTH | S_ISVTX, "t"), (S_ISVTX, "T"), (S_IXOTH, "x")],
];

/// Convert a file's mode to a string of the form '-rwxrwxrwx'.
pub fn filemode(mode: u32) -> String {
    let mut perm: Vec<&str> = Vec::new();

    for &table in FILEMODE_TABLE {
        let mut found = false;

        for &(bit, c) in table {
            if (mode & bit) == bit {
                found = true;
                perm.push(c);
                break;
            }
        }

        if !found {
            perm.push("-");
        }
    }

    perm.join("")
}
