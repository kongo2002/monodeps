# monodeps

[![monodeps](https://github.com/kongo2002/monodeps/actions/workflows/build.yaml/badge.svg)][actions]

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


## Idea

The general idea of monodeps is to calculate direct and transitive dependencies
of services by both using `Depsfile` files that explicitly list dependencies and
also trying to auto-discover dependencies for a list of known programming
languages/frameworks. Of course, auto-discovery can only be considered "best
effort" but can ease and simplify the initial setup of a mono-repository
considerably.


### Auto discovery

One of the ideas of *monodeps* is to come with "batteries included" as much as
as possible. For that purpose, auto discovery of service's dependencies is one
of the key components.

As of now, the following languages/frameworks are supported:

- Go
- Flutter/Dart
- C\# (.NET)
- Kustomize

By default, *monodeps* will try to auto-detect the mentioned language/frameworks
based on the present files and file types. Once detected, the language specific
dependency discovery will attempt to automatically add dependencies to the ones
that are explicitly listed in the `Depsfile`.


### Service discovery

In order to know what directories are build units, *monodeps* will determine
which folders are potential "service roots". By default, every folder that
contains a `Depsfile` will be considered a "service".

Moreover you can toggle via `--makefile` and `--justfile` to also consider every
folder that contains either a `Makefile` or `justfile` as service roots
respectively. Of course, in that case you are solely relying on the
auto-discovery feature to determine dependencies.


## Installation

Go to the [releases page][releases], expand the list of assets and download a
ready-to-run binary.


## Configuration
c
In terms of configuration there are two components: a global, optional
configuration file on a per-repository base and a `Depsfile` for each service
that is built and deployed in CI/CD.


### Depsfile

The `Depsfile` is expected in the root directory of a single service/deployment.
All components of the file are optional, meaning an empty file can work as well,
consequently fully depending on monodeps' auto-discovery capabilities.

```yaml
# Following you can list directories of other services or files this particular
# service is depending on. Whenever any of these files or directories are part
# of the changed input files, this service will be candidate for build and
# publish.
dependencies:
  - ../../services/auth-service
  - ../../shared/postgres
  - ../../shared/pagination

# Specifying the language of the respective service helps monodeps to know what
# files to look for in terms of auto-discovering dependencies. Otherwise,
# monodeps will try to guess the language based on the majority of files in the
# service directory.
#
# Currently supported: go/golang, csharp/dotnet, dart/flutter, kustomize
languages:
  - go
```


### Global configuration

The global configuration file, `.monodeps.yaml`, is by default expected in the
mono-repository root directory. The file is optional but allows to configure a
few additional aspects of monodeps:

```yaml
# You can specify a list of "global" dependencies that means a change to any of
# the listed files/directories will cause a build and publish of *every* service
# known to monodeps.
global_dependencies:
  - ./shared

# You can tweak the behavior of auto-discovered dependencies of particular
# languages.
auto_discovery:
  go:
    # You have to specify the valid prefixes of go modules that identify the
    # dependencies of other services/packages in this mono-repository.
    #
    # This setting is *required* for go auto-discovery of dependencies to work!
    package_prefixes:
      - dev.my.org/services
  dotnet:
    # Similarly, you can configure relevant namespace(s) that should be
    # considered relevant to this mono-repository.
    #
    # This setting is *optional*.
    package_namespaces:
      - MyOrganization.Services
```


## Building

*monodeps* is written in Rust and can be built using the usual `cargo`
toolchain:

```console
$ cargo build --release
```


[actions]: https://github.com/kongo2002/monodeps/actions/
[releases]: https://github.com/kongo2002/monodeps/releases/
