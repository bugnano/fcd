# The FCD File Manager (FranCommanDer)

The FCD File Manager (FranCommanDer) is a text based file manager that is the
continuation of rnr (RNR's Not Ranger). It combines the best features of
[Midnight Commander]( https://midnight-commander.org/ ) and
[Ranger](https://ranger.github.io/),
all in a single executable file.

Its main goals are:

* To be the most robust file copier in existence
* To be the handiest file manager when it comes to finding on which
  files/directories to operate


## Features

* Portable single file executable, compatible with any Linux distribution
  released from 2014 onwards, if compiled against musl libc (such as the packages
  found in the GitHub Releases) (Tested as far back as Centos 7 and Ubuntu 14.04)
* Very fast file and directory browser with Vim-style keys and powerful fuzzy
  filter and fuzzy finder
* Explore compressed archives as normal read-only directories (requires
  [archivefs](https://github.com/bugnano/archivefs) or archivemount)
* Fast directory jumping with bookmarks
* Many file rename options
* Robust file copy engine with minimal user interaction. Great for copying
  large amounts of data reliably.
* Text and binary file viewer with line numbers and syntax highlighting for
  text, and masked data for binary, with optional hex display mode for both
  formats
* Optional file and directory preview in the other panel
* If the internal file viewer is not used, view files with the selected pager
  (default: less)
* Edit files with the selected editor (default: vi)
* Open files with the selected opener (default: xdg-open)
* Execute shell commands, with macro substitutions to easily manipulate the
  tagged files
* cd to the last visited directory on exit (compatible with bash and fish)

## Screenshots

![mc-like](https://raw.githubusercontent.com/bugnano/fcd/master/doc/mc-like.png)

![ranger-like](https://raw.githubusercontent.com/bugnano/fcd/master/doc/ranger-like.png)

## Video Tutorials

Given that fcd is the spiritual successor to rnr, the video tutorials for rnr
still mostly apply to fcd as well.

https://www.youtube.com/watch?v=dHh-7hX6dRY

https://www.youtube.com/watch?v=17-K43Z2XcU (Italian)

## System requirements

### For running

* Linux (a POSIX-compatible OS like macOS, FreeBSD or Cygwin may work, but
  it's not officially supported)

Yes, that's it, no other dependencies required.

### For compiling

* The Rust compiler + Cargo
* A C compiler (some dependencies are in C)

### For building the man page

* Ruby
* Asciidoctor

### For compressed archive support
* [archivefs](https://github.com/bugnano/archivefs) (Recommended), or
  archivemount (Much slower and somewhat buggier than archivefs)

## Installation and running

### Prebuilt binary

Put the `fcd` executable downloaded from the GitHub Releases in a directory in
your PATH (for example in `~/.local/bin`), and run:

```bash
fcd
```

### Build from source

```bash
# This will install only the fcd executable in $HOME/.cargo/bin, without any
# man page or shell script.
# Also, make sure that the $HOME/.cargo/bin directory is in your PATH
cargo install fcd

# Alternatively, you can build from source using the command
cargo build --release
# and then copy the file target/release/fcd somewhere in your PATH

# To build the man pages:
asciidoctor -b manpage doc/fcd.1.adoc
asciidoctor -b manpage doc/fcd-view.1.adoc
# and then copy the file doc/fcd.1 in a man path (like $HOME/.local/share/man/man1 )
```

### CD on exit (bash)

If you're using bash and you want to change directory on exit, first put the
`fcd.sh` file in `~/.local/share/fcd`, then you have to add a line like this in
your `~/.bashrc`:

```bash
source ~/.local/share/fcd/fcd.sh
```

### CD on exit (fish)

If you're using fish, then simply copy the file `fcd.fish` to
`~/.config/fish/functions/` (create the directory if it does not exist).

Note that this script requires at least fish version 3.1.0

## Documentation

The fcd man page can be invoked with the command:

```bash
man fcd
```

[Here is a text version of the man page](https://github.com/bugnano/fcd/blob/master/doc/fcd.1.adoc)

## Robust file copy

File copying looks like a simple operation, but there are many cases where it could go wrong.

To better understand the situation, let me tell you a couple of stories:

You have several big, multi-gigabyte files that you need to copy from one hard
drive to another.  This operation is very time consuming, so you start the
copy process in the evening, and let it run overnight.

The next day, you wake up, and see that the copy process is stuck at 10% and
you see a window prompting you what to do, as there already is a file with the
same name in the destination directory (or an error has occurred during the
copy, and the program is asking you if you want to continue or abort).

Result: you wasted almost the whole night, as the copy process was waiting for
your input.

Now imagine instead that you wake up and see that your computer shows an empty
desktop because the power went down in the night.

Result: The copy process has been interrupted and you have no idea which files
have been copied and which files not.

> There must be a better way! - Raymond Hettinger

So fcd addresses these problems in 2 ways:

1. The copy operation is completely non-interactive, the action to be done in
   case of conflict is decided before the copy process starts. Once the copy
   process starts, all the conflicts are handled automatically, and all the
   errors are skipped. At the end of the process, you will see a report window
   that shows all the actions taken by the copy engine (for example
   renaming/overwriting a file, or skipping a file due to an error). The
   report can be saved to a text file, and analized as required.
2. Every file operation is logged to a on-disk database, so when the power
   goes off (and it will...), you will know where the copy process was at, and
   resume from that.

Now, let's address the elephant in the room: The on-disk database slows down
operations considerably in the case of many small files.

While fcd defaults to using a database file, it is in fact optional, and can
be disabled by a command line switch, or by the "No DB" button.

Of course, everything said about the file copy is applied to the file move
operation as well.

## Find on which file to operate

Many times, you already know which file you want, but it's buried among
hundreds of files in the same directory, and on any other file manager
reaching the file you want is tedious.
FCD allows you to filter the directory listing by pressing the letter `f`,
followed by part of the name of the file that you want, and get to your file
very quickly.

Other times, you know which file you want, but you don't remember in which
directory it's stored.
You can find it by pressing `CTRL-P` and typing part of the name of the file.
The dialog opened with `CTRL-P` will search for the file in all the
subdirectories of the current panel, in a way that is very reminescent of the
FZF tool.

## Non-Goals

* Transfer Speed: In the speed/reliability tradeoff it will choose reliability first.
* Portability: It is intended for use in Linux, and, although it may work on
  other POSIX-compatible operating systems, errors on non-Linux systems are not
  considered bugs.
* Configurability: Apart from choosing the pager, opener and editor, a colour
  scheme and custom bookmarks, it is not intended to be configurable, so no
  custom commands or keybindings.  This has the advantage that fcd will work the
  same everywhere it is installed.

## Note for packagers

If you're packaging fcd for your distribution, consider copying the `fcd.sh`
file to the `/etc/profile.d` directory, and the `fcd.fish` file to the
`/etc/fish/functions` directory, so that fcd automatically changes directory on
exit, without needing manual configuration.

## License

GPL-3.0-or-later

