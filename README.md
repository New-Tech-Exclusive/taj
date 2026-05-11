# Taj

Taj is an experimental package manager that builds packages from source. It installs from a base mirror (via `.taj` files) or directly from git URLs (GitHub, GitLab, or any git remote).

## Quick start

```bash
cargo build --release

# install from base mirror
sudo ./target/release/taj -i PACKAGENAME

# install from GitHub
sudo ./target/release/taj -gh https://github.com/org/repo.git

# install from GitLab
sudo ./target/release/taj -gl https://gitlab.com/org/repo.git
```

## CLI

```bash
# Base mirror
taj install PACKAGENAME

# Meta/bundle packages
taj meta PACKAGENAME
taj bundle PACKAGENAME

# Direct git installs
taj install --github https://github.com/org/repo.git
taj install --gitlab https://gitlab.com/org/repo.git

# Uninstall
taj uninstall PACKAGENAME

# List installed packages
taj list

# Legacy flags (supported for compatibility)
taj -i PACKAGENAME
taj -gh https://github.com/org/repo.git
taj -gl https://gitlab.com/org/repo.git
taj -u PACKAGENAME
```

## Base mirror

By default, Taj looks for packages in the mirror repo configured in `config.toml`. Each package is described by a `.taj` file that points to a git repo and build method.

## Taj file format

Taj files are TOML. Example:

```toml
name = "ripgrep"
repo = "https://github.com/BurntSushi/ripgrep.git"
build = "cargo"
bin = "rg"
build_args = ["--features", "pcre2"]
build_dir = "build"
subdir = ""

[env]
RUSTFLAGS = "-C target-cpu=native"
```

Supported `build` values: `cargo`, `make`, `cmake`, `autotools`, `meson`, `go`, `gcc`, `g++`, `rustc`.
Use `build = "meta"` for meta packages that only install dependencies.

## Config

Taj creates a config file on first run:

- Root: `/etc/taj/config.toml`
- User: `~/.config/taj/config.toml`

```toml
mirror_repo = "https://github.com/taj-pm/packages"
mirror_branch = "main"
mirror_packages_dir = "packages"
install_dir = "/usr/local/bin"
cache_dir = "/home/you/.cache/taj"
state_file = "/var/lib/taj/installed.json"
```

## Build detection (direct git)

Taj checks for build systems in this order:

1. Cargo (`Cargo.toml`)
2. Go (`go.mod`)
3. Meson (`meson.build`)
4. CMake (`CMakeLists.txt`)
5. Make (`Makefile` or `makefile`)
6. C++ sources (`.cpp`, `.cc`, `.cxx`)
7. C sources (`.c`)
8. Rust (`src/main.rs` or `main.rs`)

## Notes

- Taj requires `git` and the relevant build tools (`cargo`, `go`, `cmake`, `make`, `gcc`, `g++`, or `rustc`).
- When installing to system paths like `/usr/local/bin`, run with `sudo`.
