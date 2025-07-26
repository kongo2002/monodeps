# monodeps

[![monodeps](https://github.com/kongo2002/monodeps/actions/workflows/build.yml/badge.svg)][actions]

monodeps is a tool to help with change detection in mono-repository setups in
order to determine which services or folders are candidate for build and publish
in CI/CD environments.

The program expects a list of changed/updated files on STDIN. These files are
the base for the change detection algorithm. The program output will be all
services/folders that have to be built, based on the respective `Depsfile` files
in each service folder.

For instance, you could pipe the git diff output to monodeps:

```console
git diff-tree --no-commit-id --name-only HEAD -r | monodeps
```


## Installation

Go to the [releases page][releases], expand the list of assets and download a
ready-to-run binary.


## Building

*monodeps* is written in Rust and can be built using the usual `cargo`
toolchain:

```console
$ cargo build --release
```


[actions]: https://github.com/kongo2002/monodeps/actions/
[releases]: https://github.com/kongo2002/monodeps/releases/
