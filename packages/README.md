# Packages

This folder contains Taj package definitions. Each package is a TOML file named after the package with a .taj extension.

Required fields:
- repo
- build

Optional fields:
- name
- category
- bin
- build_args
- subdir
- env (map of environment variables)
- deps (dependency metadata)
- depends (mirror package dependencies)
- ref (git ref pin)
- install (install step for libraries)

Examples are provided in this folder.

## categories

Suggested categories (use what fits your distro):

- base
- shell
- filesystem
- networking
- compression
- security
- editors
- utils
- monitoring
- devtools
- libraries

## install

Use install to run a package's install step into a staging directory, then copy
files into the system prefix. Supported methods: `make`, `cmake`.

The install prefix defaults to the parent of `install_dir` in config.

Optional fields: `args`, `env`, `prefix`, `destdir`.

Example:

```toml
[install]
method = "make"
args = ["install"]
```

## deps

Use deps to provide build-time checks and install hints. Taj will fail early if
required tools or pkg-config libraries are missing. The keys under
`[deps.packages]` are free-form; use your distro's package manager name or a
`manual` key for custom instructions.

Example:

```toml
[deps]
tools = ["cmake", "ninja"]
pkg_config = ["ncurses"]
message = "Install a terminal library and headers for ncurses"

[deps.packages]
manual = ["ncurses (dev headers)", "cmake", "ninja"]
```

# Contributing packages

To contribute a package, just make a .taj file that has the requirements and any optional fields.
Any expieremental, nightly, or beta packages must be put under a seperate taj file that has the name of the file, followed by its branch
Example:

```bash
# These probably don't exist, they are just examples

fd-beta.taj
ripgrep-nightly.taj

```

## depends

Use depends to install other mirror packages before building this one. Entries
must match the `.taj` file name (without the extension).

Example:

```toml
depends = ["zlib", "openssl"]
```

## ref

Use ref to pin a package to a tag, branch, or commit.

Example:

```toml
ref = "v1.2.3"
```