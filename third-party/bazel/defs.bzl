###############################################################################
# @generated
# DO NOT MODIFY: This file is auto-generated by a crate_universe tool. To
# regenerate this file, run the following:
#
#     bazel run @@//third-party:vendor
###############################################################################
"""
# `crates_repository` API

- [aliases](#aliases)
- [crate_deps](#crate_deps)
- [all_crate_deps](#all_crate_deps)
- [crate_repositories](#crate_repositories)

"""

load("@bazel_skylib//lib:selects.bzl", "selects")
load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")
load("@bazel_tools//tools/build_defs/repo:utils.bzl", "maybe")

###############################################################################
# MACROS API
###############################################################################

# An identifier that represent common dependencies (unconditional).
_COMMON_CONDITION = ""

def _flatten_dependency_maps(all_dependency_maps):
    """Flatten a list of dependency maps into one dictionary.

    Dependency maps have the following structure:

    ```python
    DEPENDENCIES_MAP = {
        # The first key in the map is a Bazel package
        # name of the workspace this file is defined in.
        "workspace_member_package": {

            # Not all dependencies are supported for all platforms.
            # the condition key is the condition required to be true
            # on the host platform.
            "condition": {

                # An alias to a crate target.     # The label of the crate target the
                # Aliases are only crate names.   # package name refers to.
                "package_name":                   "@full//:label",
            }
        }
    }
    ```

    Args:
        all_dependency_maps (list): A list of dicts as described above

    Returns:
        dict: A dictionary as described above
    """
    dependencies = {}

    for workspace_deps_map in all_dependency_maps:
        for pkg_name, conditional_deps_map in workspace_deps_map.items():
            if pkg_name not in dependencies:
                non_frozen_map = dict()
                for key, values in conditional_deps_map.items():
                    non_frozen_map.update({key: dict(values.items())})
                dependencies.setdefault(pkg_name, non_frozen_map)
                continue

            for condition, deps_map in conditional_deps_map.items():
                # If the condition has not been recorded, do so and continue
                if condition not in dependencies[pkg_name]:
                    dependencies[pkg_name].setdefault(condition, dict(deps_map.items()))
                    continue

                # Alert on any miss-matched dependencies
                inconsistent_entries = []
                for crate_name, crate_label in deps_map.items():
                    existing = dependencies[pkg_name][condition].get(crate_name)
                    if existing and existing != crate_label:
                        inconsistent_entries.append((crate_name, existing, crate_label))
                    dependencies[pkg_name][condition].update({crate_name: crate_label})

    return dependencies

def crate_deps(deps, package_name = None):
    """Finds the fully qualified label of the requested crates for the package where this macro is called.

    Args:
        deps (list): The desired list of crate targets.
        package_name (str, optional): The package name of the set of dependencies to look up.
            Defaults to `native.package_name()`.

    Returns:
        list: A list of labels to generated rust targets (str)
    """

    if not deps:
        return []

    if package_name == None:
        package_name = native.package_name()

    # Join both sets of dependencies
    dependencies = _flatten_dependency_maps([
        _NORMAL_DEPENDENCIES,
        _NORMAL_DEV_DEPENDENCIES,
        _PROC_MACRO_DEPENDENCIES,
        _PROC_MACRO_DEV_DEPENDENCIES,
        _BUILD_DEPENDENCIES,
        _BUILD_PROC_MACRO_DEPENDENCIES,
    ]).pop(package_name, {})

    # Combine all conditional packages so we can easily index over a flat list
    # TODO: Perhaps this should actually return select statements and maintain
    # the conditionals of the dependencies
    flat_deps = {}
    for deps_set in dependencies.values():
        for crate_name, crate_label in deps_set.items():
            flat_deps.update({crate_name: crate_label})

    missing_crates = []
    crate_targets = []
    for crate_target in deps:
        if crate_target not in flat_deps:
            missing_crates.append(crate_target)
        else:
            crate_targets.append(flat_deps[crate_target])

    if missing_crates:
        fail("Could not find crates `{}` among dependencies of `{}`. Available dependencies were `{}`".format(
            missing_crates,
            package_name,
            dependencies,
        ))

    return crate_targets

def all_crate_deps(
        normal = False,
        normal_dev = False,
        proc_macro = False,
        proc_macro_dev = False,
        build = False,
        build_proc_macro = False,
        package_name = None):
    """Finds the fully qualified label of all requested direct crate dependencies \
    for the package where this macro is called.

    If no parameters are set, all normal dependencies are returned. Setting any one flag will
    otherwise impact the contents of the returned list.

    Args:
        normal (bool, optional): If True, normal dependencies are included in the
            output list.
        normal_dev (bool, optional): If True, normal dev dependencies will be
            included in the output list..
        proc_macro (bool, optional): If True, proc_macro dependencies are included
            in the output list.
        proc_macro_dev (bool, optional): If True, dev proc_macro dependencies are
            included in the output list.
        build (bool, optional): If True, build dependencies are included
            in the output list.
        build_proc_macro (bool, optional): If True, build proc_macro dependencies are
            included in the output list.
        package_name (str, optional): The package name of the set of dependencies to look up.
            Defaults to `native.package_name()` when unset.

    Returns:
        list: A list of labels to generated rust targets (str)
    """

    if package_name == None:
        package_name = native.package_name()

    # Determine the relevant maps to use
    all_dependency_maps = []
    if normal:
        all_dependency_maps.append(_NORMAL_DEPENDENCIES)
    if normal_dev:
        all_dependency_maps.append(_NORMAL_DEV_DEPENDENCIES)
    if proc_macro:
        all_dependency_maps.append(_PROC_MACRO_DEPENDENCIES)
    if proc_macro_dev:
        all_dependency_maps.append(_PROC_MACRO_DEV_DEPENDENCIES)
    if build:
        all_dependency_maps.append(_BUILD_DEPENDENCIES)
    if build_proc_macro:
        all_dependency_maps.append(_BUILD_PROC_MACRO_DEPENDENCIES)

    # Default to always using normal dependencies
    if not all_dependency_maps:
        all_dependency_maps.append(_NORMAL_DEPENDENCIES)

    dependencies = _flatten_dependency_maps(all_dependency_maps).pop(package_name, None)

    if not dependencies:
        if dependencies == None:
            fail("Tried to get all_crate_deps for package " + package_name + " but that package had no Cargo.toml file")
        else:
            return []

    crate_deps = list(dependencies.pop(_COMMON_CONDITION, {}).values())
    for condition, deps in dependencies.items():
        crate_deps += selects.with_or({
            tuple(_CONDITIONS[condition]): deps.values(),
            "//conditions:default": [],
        })

    return crate_deps

def aliases(
        normal = False,
        normal_dev = False,
        proc_macro = False,
        proc_macro_dev = False,
        build = False,
        build_proc_macro = False,
        package_name = None):
    """Produces a map of Crate alias names to their original label

    If no dependency kinds are specified, `normal` and `proc_macro` are used by default.
    Setting any one flag will otherwise determine the contents of the returned dict.

    Args:
        normal (bool, optional): If True, normal dependencies are included in the
            output list.
        normal_dev (bool, optional): If True, normal dev dependencies will be
            included in the output list..
        proc_macro (bool, optional): If True, proc_macro dependencies are included
            in the output list.
        proc_macro_dev (bool, optional): If True, dev proc_macro dependencies are
            included in the output list.
        build (bool, optional): If True, build dependencies are included
            in the output list.
        build_proc_macro (bool, optional): If True, build proc_macro dependencies are
            included in the output list.
        package_name (str, optional): The package name of the set of dependencies to look up.
            Defaults to `native.package_name()` when unset.

    Returns:
        dict: The aliases of all associated packages
    """
    if package_name == None:
        package_name = native.package_name()

    # Determine the relevant maps to use
    all_aliases_maps = []
    if normal:
        all_aliases_maps.append(_NORMAL_ALIASES)
    if normal_dev:
        all_aliases_maps.append(_NORMAL_DEV_ALIASES)
    if proc_macro:
        all_aliases_maps.append(_PROC_MACRO_ALIASES)
    if proc_macro_dev:
        all_aliases_maps.append(_PROC_MACRO_DEV_ALIASES)
    if build:
        all_aliases_maps.append(_BUILD_ALIASES)
    if build_proc_macro:
        all_aliases_maps.append(_BUILD_PROC_MACRO_ALIASES)

    # Default to always using normal aliases
    if not all_aliases_maps:
        all_aliases_maps.append(_NORMAL_ALIASES)
        all_aliases_maps.append(_PROC_MACRO_ALIASES)

    aliases = _flatten_dependency_maps(all_aliases_maps).pop(package_name, None)

    if not aliases:
        return dict()

    common_items = aliases.pop(_COMMON_CONDITION, {}).items()

    # If there are only common items in the dictionary, immediately return them
    if not len(aliases.keys()) == 1:
        return dict(common_items)

    # Build a single select statement where each conditional has accounted for the
    # common set of aliases.
    crate_aliases = {"//conditions:default": dict(common_items)}
    for condition, deps in aliases.items():
        condition_triples = _CONDITIONS[condition]
        for triple in condition_triples:
            if triple in crate_aliases:
                crate_aliases[triple].update(deps)
            else:
                crate_aliases.update({triple: dict(deps.items() + common_items)})

    return select(crate_aliases)

###############################################################################
# WORKSPACE MEMBER DEPS AND ALIASES
###############################################################################

_NORMAL_DEPENDENCIES = {
    "third-party": {
        _COMMON_CONDITION: {
            "cc": Label("@vendor__cc-1.0.98//:cc"),
            "clap": Label("@vendor__clap-4.5.4//:clap"),
            "codespan-reporting": Label("@vendor__codespan-reporting-0.11.1//:codespan_reporting"),
            "once_cell": Label("@vendor__once_cell-1.19.0//:once_cell"),
            "proc-macro2": Label("@vendor__proc-macro2-1.0.85//:proc_macro2"),
            "quote": Label("@vendor__quote-1.0.36//:quote"),
            "scratch": Label("@vendor__scratch-1.0.7//:scratch"),
            "syn": Label("@vendor__syn-2.0.66//:syn"),
        },
    },
}

_NORMAL_ALIASES = {
    "third-party": {
        _COMMON_CONDITION: {
        },
    },
}

_NORMAL_DEV_DEPENDENCIES = {
    "third-party": {
    },
}

_NORMAL_DEV_ALIASES = {
    "third-party": {
    },
}

_PROC_MACRO_DEPENDENCIES = {
    "third-party": {
    },
}

_PROC_MACRO_ALIASES = {
    "third-party": {
    },
}

_PROC_MACRO_DEV_DEPENDENCIES = {
    "third-party": {
    },
}

_PROC_MACRO_DEV_ALIASES = {
    "third-party": {
    },
}

_BUILD_DEPENDENCIES = {
    "third-party": {
    },
}

_BUILD_ALIASES = {
    "third-party": {
    },
}

_BUILD_PROC_MACRO_DEPENDENCIES = {
    "third-party": {
    },
}

_BUILD_PROC_MACRO_ALIASES = {
    "third-party": {
    },
}

_CONDITIONS = {
    "aarch64-apple-darwin": ["@rules_rust//rust/platform:aarch64-apple-darwin"],
    "aarch64-apple-ios": ["@rules_rust//rust/platform:aarch64-apple-ios"],
    "aarch64-apple-ios-sim": ["@rules_rust//rust/platform:aarch64-apple-ios-sim"],
    "aarch64-fuchsia": ["@rules_rust//rust/platform:aarch64-fuchsia"],
    "aarch64-linux-android": ["@rules_rust//rust/platform:aarch64-linux-android"],
    "aarch64-pc-windows-gnullvm": [],
    "aarch64-pc-windows-msvc": ["@rules_rust//rust/platform:aarch64-pc-windows-msvc"],
    "aarch64-unknown-linux-gnu": ["@rules_rust//rust/platform:aarch64-unknown-linux-gnu"],
    "aarch64-unknown-nixos-gnu": ["@rules_rust//rust/platform:aarch64-unknown-nixos-gnu"],
    "aarch64-unknown-nto-qnx710": ["@rules_rust//rust/platform:aarch64-unknown-nto-qnx710"],
    "arm-unknown-linux-gnueabi": ["@rules_rust//rust/platform:arm-unknown-linux-gnueabi"],
    "armv7-linux-androideabi": ["@rules_rust//rust/platform:armv7-linux-androideabi"],
    "armv7-unknown-linux-gnueabi": ["@rules_rust//rust/platform:armv7-unknown-linux-gnueabi"],
    "cfg(all(any(target_arch = \"x86_64\", target_arch = \"arm64ec\"), target_env = \"msvc\", not(windows_raw_dylib)))": ["@rules_rust//rust/platform:x86_64-pc-windows-msvc"],
    "cfg(all(target_arch = \"aarch64\", target_env = \"msvc\", not(windows_raw_dylib)))": ["@rules_rust//rust/platform:aarch64-pc-windows-msvc"],
    "cfg(all(target_arch = \"x86\", target_env = \"gnu\", not(target_abi = \"llvm\"), not(windows_raw_dylib)))": ["@rules_rust//rust/platform:i686-unknown-linux-gnu"],
    "cfg(all(target_arch = \"x86\", target_env = \"msvc\", not(windows_raw_dylib)))": ["@rules_rust//rust/platform:i686-pc-windows-msvc"],
    "cfg(all(target_arch = \"x86_64\", target_env = \"gnu\", not(target_abi = \"llvm\"), not(windows_raw_dylib)))": ["@rules_rust//rust/platform:x86_64-unknown-linux-gnu", "@rules_rust//rust/platform:x86_64-unknown-nixos-gnu"],
    "cfg(windows)": ["@rules_rust//rust/platform:aarch64-pc-windows-msvc", "@rules_rust//rust/platform:i686-pc-windows-msvc", "@rules_rust//rust/platform:x86_64-pc-windows-msvc"],
    "i686-apple-darwin": ["@rules_rust//rust/platform:i686-apple-darwin"],
    "i686-linux-android": ["@rules_rust//rust/platform:i686-linux-android"],
    "i686-pc-windows-gnullvm": [],
    "i686-pc-windows-msvc": ["@rules_rust//rust/platform:i686-pc-windows-msvc"],
    "i686-unknown-freebsd": ["@rules_rust//rust/platform:i686-unknown-freebsd"],
    "i686-unknown-linux-gnu": ["@rules_rust//rust/platform:i686-unknown-linux-gnu"],
    "powerpc-unknown-linux-gnu": ["@rules_rust//rust/platform:powerpc-unknown-linux-gnu"],
    "riscv32imc-unknown-none-elf": ["@rules_rust//rust/platform:riscv32imc-unknown-none-elf"],
    "riscv64gc-unknown-none-elf": ["@rules_rust//rust/platform:riscv64gc-unknown-none-elf"],
    "s390x-unknown-linux-gnu": ["@rules_rust//rust/platform:s390x-unknown-linux-gnu"],
    "thumbv7em-none-eabi": ["@rules_rust//rust/platform:thumbv7em-none-eabi"],
    "thumbv8m.main-none-eabi": ["@rules_rust//rust/platform:thumbv8m.main-none-eabi"],
    "wasm32-unknown-unknown": ["@rules_rust//rust/platform:wasm32-unknown-unknown"],
    "wasm32-wasi": ["@rules_rust//rust/platform:wasm32-wasi"],
    "x86_64-apple-darwin": ["@rules_rust//rust/platform:x86_64-apple-darwin"],
    "x86_64-apple-ios": ["@rules_rust//rust/platform:x86_64-apple-ios"],
    "x86_64-fuchsia": ["@rules_rust//rust/platform:x86_64-fuchsia"],
    "x86_64-linux-android": ["@rules_rust//rust/platform:x86_64-linux-android"],
    "x86_64-pc-windows-gnullvm": [],
    "x86_64-pc-windows-msvc": ["@rules_rust//rust/platform:x86_64-pc-windows-msvc"],
    "x86_64-unknown-freebsd": ["@rules_rust//rust/platform:x86_64-unknown-freebsd"],
    "x86_64-unknown-linux-gnu": ["@rules_rust//rust/platform:x86_64-unknown-linux-gnu"],
    "x86_64-unknown-nixos-gnu": ["@rules_rust//rust/platform:x86_64-unknown-nixos-gnu"],
    "x86_64-unknown-none": ["@rules_rust//rust/platform:x86_64-unknown-none"],
}

###############################################################################

def crate_repositories():
    """A macro for defining repositories for all generated crates.

    Returns:
      A list of repos visible to the module through the module extension.
    """
    maybe(
        http_archive,
        name = "vendor__anstyle-1.0.7",
        sha256 = "038dfcf04a5feb68e9c60b21c9625a54c2c0616e79b72b0fd87075a056ae1d1b",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/anstyle/1.0.7/download"],
        strip_prefix = "anstyle-1.0.7",
        build_file = Label("@//third-party/bazel:BUILD.anstyle-1.0.7.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__cc-1.0.98",
        sha256 = "41c270e7540d725e65ac7f1b212ac8ce349719624d7bcff99f8e2e488e8cf03f",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/cc/1.0.98/download"],
        strip_prefix = "cc-1.0.98",
        build_file = Label("@//third-party/bazel:BUILD.cc-1.0.98.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__clap-4.5.4",
        sha256 = "90bc066a67923782aa8515dbaea16946c5bcc5addbd668bb80af688e53e548a0",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/clap/4.5.4/download"],
        strip_prefix = "clap-4.5.4",
        build_file = Label("@//third-party/bazel:BUILD.clap-4.5.4.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__clap_builder-4.5.2",
        sha256 = "ae129e2e766ae0ec03484e609954119f123cc1fe650337e155d03b022f24f7b4",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/clap_builder/4.5.2/download"],
        strip_prefix = "clap_builder-4.5.2",
        build_file = Label("@//third-party/bazel:BUILD.clap_builder-4.5.2.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__clap_lex-0.7.0",
        sha256 = "98cc8fbded0c607b7ba9dd60cd98df59af97e84d24e49c8557331cfc26d301ce",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/clap_lex/0.7.0/download"],
        strip_prefix = "clap_lex-0.7.0",
        build_file = Label("@//third-party/bazel:BUILD.clap_lex-0.7.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__codespan-reporting-0.11.1",
        sha256 = "3538270d33cc669650c4b093848450d380def10c331d38c768e34cac80576e6e",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/codespan-reporting/0.11.1/download"],
        strip_prefix = "codespan-reporting-0.11.1",
        build_file = Label("@//third-party/bazel:BUILD.codespan-reporting-0.11.1.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__once_cell-1.19.0",
        sha256 = "3fdb12b2476b595f9358c5161aa467c2438859caa136dec86c26fdd2efe17b92",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/once_cell/1.19.0/download"],
        strip_prefix = "once_cell-1.19.0",
        build_file = Label("@//third-party/bazel:BUILD.once_cell-1.19.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__proc-macro2-1.0.85",
        sha256 = "22244ce15aa966053a896d1accb3a6e68469b97c7f33f284b99f0d576879fc23",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/proc-macro2/1.0.85/download"],
        strip_prefix = "proc-macro2-1.0.85",
        build_file = Label("@//third-party/bazel:BUILD.proc-macro2-1.0.85.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__quote-1.0.36",
        sha256 = "0fa76aaf39101c457836aec0ce2316dbdc3ab723cdda1c6bd4e6ad4208acaca7",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/quote/1.0.36/download"],
        strip_prefix = "quote-1.0.36",
        build_file = Label("@//third-party/bazel:BUILD.quote-1.0.36.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__scratch-1.0.7",
        sha256 = "a3cf7c11c38cb994f3d40e8a8cde3bbd1f72a435e4c49e85d6553d8312306152",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/scratch/1.0.7/download"],
        strip_prefix = "scratch-1.0.7",
        build_file = Label("@//third-party/bazel:BUILD.scratch-1.0.7.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__syn-2.0.66",
        sha256 = "c42f3f41a2de00b01c0aaad383c5a45241efc8b2d1eda5661812fda5f3cdcff5",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/syn/2.0.66/download"],
        strip_prefix = "syn-2.0.66",
        build_file = Label("@//third-party/bazel:BUILD.syn-2.0.66.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__termcolor-1.4.1",
        sha256 = "06794f8f6c5c898b3275aebefa6b8a1cb24cd2c6c79397ab15774837a0bc5755",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/termcolor/1.4.1/download"],
        strip_prefix = "termcolor-1.4.1",
        build_file = Label("@//third-party/bazel:BUILD.termcolor-1.4.1.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__unicode-ident-1.0.12",
        sha256 = "3354b9ac3fae1ff6755cb6db53683adb661634f67557942dea4facebec0fee4b",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/unicode-ident/1.0.12/download"],
        strip_prefix = "unicode-ident-1.0.12",
        build_file = Label("@//third-party/bazel:BUILD.unicode-ident-1.0.12.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__unicode-width-0.1.13",
        sha256 = "0336d538f7abc86d282a4189614dfaa90810dfc2c6f6427eaf88e16311dd225d",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/unicode-width/0.1.13/download"],
        strip_prefix = "unicode-width-0.1.13",
        build_file = Label("@//third-party/bazel:BUILD.unicode-width-0.1.13.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__winapi-util-0.1.8",
        sha256 = "4d4cc384e1e73b93bafa6fb4f1df8c41695c8a91cf9c4c64358067d15a7b6c6b",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/winapi-util/0.1.8/download"],
        strip_prefix = "winapi-util-0.1.8",
        build_file = Label("@//third-party/bazel:BUILD.winapi-util-0.1.8.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows-sys-0.52.0",
        sha256 = "282be5f36a8ce781fad8c8ae18fa3f9beff57ec1b52cb3de0789201425d9a33d",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows-sys/0.52.0/download"],
        strip_prefix = "windows-sys-0.52.0",
        build_file = Label("@//third-party/bazel:BUILD.windows-sys-0.52.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows-targets-0.52.5",
        sha256 = "6f0713a46559409d202e70e28227288446bf7841d3211583a4b53e3f6d96e7eb",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows-targets/0.52.5/download"],
        strip_prefix = "windows-targets-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows-targets-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_aarch64_gnullvm-0.52.5",
        sha256 = "7088eed71e8b8dda258ecc8bac5fb1153c5cffaf2578fc8ff5d61e23578d3263",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_aarch64_gnullvm/0.52.5/download"],
        strip_prefix = "windows_aarch64_gnullvm-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_aarch64_gnullvm-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_aarch64_msvc-0.52.5",
        sha256 = "9985fd1504e250c615ca5f281c3f7a6da76213ebd5ccc9561496568a2752afb6",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_aarch64_msvc/0.52.5/download"],
        strip_prefix = "windows_aarch64_msvc-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_aarch64_msvc-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_i686_gnu-0.52.5",
        sha256 = "88ba073cf16d5372720ec942a8ccbf61626074c6d4dd2e745299726ce8b89670",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_i686_gnu/0.52.5/download"],
        strip_prefix = "windows_i686_gnu-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_i686_gnu-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_i686_gnullvm-0.52.5",
        sha256 = "87f4261229030a858f36b459e748ae97545d6f1ec60e5e0d6a3d32e0dc232ee9",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_i686_gnullvm/0.52.5/download"],
        strip_prefix = "windows_i686_gnullvm-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_i686_gnullvm-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_i686_msvc-0.52.5",
        sha256 = "db3c2bf3d13d5b658be73463284eaf12830ac9a26a90c717b7f771dfe97487bf",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_i686_msvc/0.52.5/download"],
        strip_prefix = "windows_i686_msvc-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_i686_msvc-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_x86_64_gnu-0.52.5",
        sha256 = "4e4246f76bdeff09eb48875a0fd3e2af6aada79d409d33011886d3e1581517d9",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_x86_64_gnu/0.52.5/download"],
        strip_prefix = "windows_x86_64_gnu-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_x86_64_gnu-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_x86_64_gnullvm-0.52.5",
        sha256 = "852298e482cd67c356ddd9570386e2862b5673c85bd5f88df9ab6802b334c596",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_x86_64_gnullvm/0.52.5/download"],
        strip_prefix = "windows_x86_64_gnullvm-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_x86_64_gnullvm-0.52.5.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__windows_x86_64_msvc-0.52.5",
        sha256 = "bec47e5bfd1bff0eeaf6d8b485cc1074891a197ab4225d504cb7a1ab88b02bf0",
        type = "tar.gz",
        urls = ["https://static.crates.io/crates/windows_x86_64_msvc/0.52.5/download"],
        strip_prefix = "windows_x86_64_msvc-0.52.5",
        build_file = Label("@//third-party/bazel:BUILD.windows_x86_64_msvc-0.52.5.bazel"),
    )

    return [
        struct(repo = "vendor__cc-1.0.98", is_dev_dep = False),
        struct(repo = "vendor__clap-4.5.4", is_dev_dep = False),
        struct(repo = "vendor__codespan-reporting-0.11.1", is_dev_dep = False),
        struct(repo = "vendor__once_cell-1.19.0", is_dev_dep = False),
        struct(repo = "vendor__proc-macro2-1.0.85", is_dev_dep = False),
        struct(repo = "vendor__quote-1.0.36", is_dev_dep = False),
        struct(repo = "vendor__scratch-1.0.7", is_dev_dep = False),
        struct(repo = "vendor__syn-2.0.66", is_dev_dep = False),
    ]
