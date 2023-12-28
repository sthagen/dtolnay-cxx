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
            "cc": "@vendor__cc-1.0.83//:cc",
            "clap": "@vendor__clap-4.4.11//:clap",
            "codespan-reporting": "@vendor__codespan-reporting-0.11.1//:codespan_reporting",
            "once_cell": "@vendor__once_cell-1.19.0//:once_cell",
            "proc-macro2": "@vendor__proc-macro2-1.0.70//:proc_macro2",
            "quote": "@vendor__quote-1.0.33//:quote",
            "scratch": "@vendor__scratch-1.0.7//:scratch",
            "syn": "@vendor__syn-2.0.41//:syn",
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
    "aarch64-pc-windows-msvc": ["@rules_rust//rust/platform:aarch64-pc-windows-msvc"],
    "aarch64-unknown-linux-gnu": ["@rules_rust//rust/platform:aarch64-unknown-linux-gnu"],
    "aarch64-unknown-nixos-gnu": ["@rules_rust//rust/platform:aarch64-unknown-nixos-gnu"],
    "aarch64-unknown-nto-qnx710": ["@rules_rust//rust/platform:aarch64-unknown-nto-qnx710"],
    "arm-unknown-linux-gnueabi": ["@rules_rust//rust/platform:arm-unknown-linux-gnueabi"],
    "armv7-linux-androideabi": ["@rules_rust//rust/platform:armv7-linux-androideabi"],
    "armv7-unknown-linux-gnueabi": ["@rules_rust//rust/platform:armv7-unknown-linux-gnueabi"],
    "cfg(unix)": ["@rules_rust//rust/platform:aarch64-apple-darwin", "@rules_rust//rust/platform:aarch64-apple-ios", "@rules_rust//rust/platform:aarch64-apple-ios-sim", "@rules_rust//rust/platform:aarch64-fuchsia", "@rules_rust//rust/platform:aarch64-linux-android", "@rules_rust//rust/platform:aarch64-unknown-linux-gnu", "@rules_rust//rust/platform:aarch64-unknown-nixos-gnu", "@rules_rust//rust/platform:aarch64-unknown-nto-qnx710", "@rules_rust//rust/platform:arm-unknown-linux-gnueabi", "@rules_rust//rust/platform:armv7-linux-androideabi", "@rules_rust//rust/platform:armv7-unknown-linux-gnueabi", "@rules_rust//rust/platform:i686-apple-darwin", "@rules_rust//rust/platform:i686-linux-android", "@rules_rust//rust/platform:i686-unknown-freebsd", "@rules_rust//rust/platform:i686-unknown-linux-gnu", "@rules_rust//rust/platform:powerpc-unknown-linux-gnu", "@rules_rust//rust/platform:s390x-unknown-linux-gnu", "@rules_rust//rust/platform:x86_64-apple-darwin", "@rules_rust//rust/platform:x86_64-apple-ios", "@rules_rust//rust/platform:x86_64-fuchsia", "@rules_rust//rust/platform:x86_64-linux-android", "@rules_rust//rust/platform:x86_64-unknown-freebsd", "@rules_rust//rust/platform:x86_64-unknown-linux-gnu", "@rules_rust//rust/platform:x86_64-unknown-nixos-gnu"],
    "cfg(windows)": ["@rules_rust//rust/platform:aarch64-pc-windows-msvc", "@rules_rust//rust/platform:i686-pc-windows-msvc", "@rules_rust//rust/platform:x86_64-pc-windows-msvc"],
    "i686-apple-darwin": ["@rules_rust//rust/platform:i686-apple-darwin"],
    "i686-linux-android": ["@rules_rust//rust/platform:i686-linux-android"],
    "i686-pc-windows-gnu": [],
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
    "x86_64-pc-windows-gnu": [],
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
        name = "vendor__anstyle-1.0.4",
        sha256 = "7079075b41f533b8c61d2a4d073c4676e1f8b249ff94a393b0595db304e0dd87",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/anstyle/1.0.4/download"],
        strip_prefix = "anstyle-1.0.4",
        build_file = Label("@//third-party/bazel:BUILD.anstyle-1.0.4.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__cc-1.0.83",
        sha256 = "f1174fb0b6ec23863f8b971027804a42614e347eafb0a95bf0b12cdae21fc4d0",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/cc/1.0.83/download"],
        strip_prefix = "cc-1.0.83",
        build_file = Label("@//third-party/bazel:BUILD.cc-1.0.83.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__clap-4.4.11",
        sha256 = "bfaff671f6b22ca62406885ece523383b9b64022e341e53e009a62ebc47a45f2",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/clap/4.4.11/download"],
        strip_prefix = "clap-4.4.11",
        build_file = Label("@//third-party/bazel:BUILD.clap-4.4.11.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__clap_builder-4.4.11",
        sha256 = "a216b506622bb1d316cd51328dce24e07bdff4a6128a47c7e7fad11878d5adbb",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/clap_builder/4.4.11/download"],
        strip_prefix = "clap_builder-4.4.11",
        build_file = Label("@//third-party/bazel:BUILD.clap_builder-4.4.11.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__clap_lex-0.6.0",
        sha256 = "702fc72eb24e5a1e48ce58027a675bc24edd52096d5397d4aea7c6dd9eca0bd1",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/clap_lex/0.6.0/download"],
        strip_prefix = "clap_lex-0.6.0",
        build_file = Label("@//third-party/bazel:BUILD.clap_lex-0.6.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__codespan-reporting-0.11.1",
        sha256 = "3538270d33cc669650c4b093848450d380def10c331d38c768e34cac80576e6e",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/codespan-reporting/0.11.1/download"],
        strip_prefix = "codespan-reporting-0.11.1",
        build_file = Label("@//third-party/bazel:BUILD.codespan-reporting-0.11.1.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__libc-0.2.151",
        sha256 = "302d7ab3130588088d277783b1e2d2e10c9e9e4a16dd9050e6ec93fb3e7048f4",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/libc/0.2.151/download"],
        strip_prefix = "libc-0.2.151",
        build_file = Label("@//third-party/bazel:BUILD.libc-0.2.151.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__once_cell-1.19.0",
        sha256 = "3fdb12b2476b595f9358c5161aa467c2438859caa136dec86c26fdd2efe17b92",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/once_cell/1.19.0/download"],
        strip_prefix = "once_cell-1.19.0",
        build_file = Label("@//third-party/bazel:BUILD.once_cell-1.19.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__proc-macro2-1.0.70",
        sha256 = "39278fbbf5fb4f646ce651690877f89d1c5811a3d4acb27700c1cb3cdb78fd3b",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/proc-macro2/1.0.70/download"],
        strip_prefix = "proc-macro2-1.0.70",
        build_file = Label("@//third-party/bazel:BUILD.proc-macro2-1.0.70.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__quote-1.0.33",
        sha256 = "5267fca4496028628a95160fc423a33e8b2e6af8a5302579e322e4b520293cae",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/quote/1.0.33/download"],
        strip_prefix = "quote-1.0.33",
        build_file = Label("@//third-party/bazel:BUILD.quote-1.0.33.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__scratch-1.0.7",
        sha256 = "a3cf7c11c38cb994f3d40e8a8cde3bbd1f72a435e4c49e85d6553d8312306152",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/scratch/1.0.7/download"],
        strip_prefix = "scratch-1.0.7",
        build_file = Label("@//third-party/bazel:BUILD.scratch-1.0.7.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__syn-2.0.41",
        sha256 = "44c8b28c477cc3bf0e7966561e3460130e1255f7a1cf71931075f1c5e7a7e269",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/syn/2.0.41/download"],
        strip_prefix = "syn-2.0.41",
        build_file = Label("@//third-party/bazel:BUILD.syn-2.0.41.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__termcolor-1.4.0",
        sha256 = "ff1bc3d3f05aff0403e8ac0d92ced918ec05b666a43f83297ccef5bea8a3d449",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/termcolor/1.4.0/download"],
        strip_prefix = "termcolor-1.4.0",
        build_file = Label("@//third-party/bazel:BUILD.termcolor-1.4.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__unicode-ident-1.0.12",
        sha256 = "3354b9ac3fae1ff6755cb6db53683adb661634f67557942dea4facebec0fee4b",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/unicode-ident/1.0.12/download"],
        strip_prefix = "unicode-ident-1.0.12",
        build_file = Label("@//third-party/bazel:BUILD.unicode-ident-1.0.12.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__unicode-width-0.1.11",
        sha256 = "e51733f11c9c4f72aa0c160008246859e340b00807569a0da0e7a1079b27ba85",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/unicode-width/0.1.11/download"],
        strip_prefix = "unicode-width-0.1.11",
        build_file = Label("@//third-party/bazel:BUILD.unicode-width-0.1.11.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__winapi-0.3.9",
        sha256 = "5c839a674fcd7a98952e593242ea400abe93992746761e38641405d28b00f419",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/winapi/0.3.9/download"],
        strip_prefix = "winapi-0.3.9",
        build_file = Label("@//third-party/bazel:BUILD.winapi-0.3.9.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__winapi-i686-pc-windows-gnu-0.4.0",
        sha256 = "ac3b87c63620426dd9b991e5ce0329eff545bccbbb34f3be09ff6fb6ab51b7b6",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/winapi-i686-pc-windows-gnu/0.4.0/download"],
        strip_prefix = "winapi-i686-pc-windows-gnu-0.4.0",
        build_file = Label("@//third-party/bazel:BUILD.winapi-i686-pc-windows-gnu-0.4.0.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__winapi-util-0.1.6",
        sha256 = "f29e6f9198ba0d26b4c9f07dbe6f9ed633e1f3d5b8b414090084349e46a52596",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/winapi-util/0.1.6/download"],
        strip_prefix = "winapi-util-0.1.6",
        build_file = Label("@//third-party/bazel:BUILD.winapi-util-0.1.6.bazel"),
    )

    maybe(
        http_archive,
        name = "vendor__winapi-x86_64-pc-windows-gnu-0.4.0",
        sha256 = "712e227841d057c1ee1cd2fb22fa7e5a5461ae8e48fa2ca79ec42cfc1931183f",
        type = "tar.gz",
        urls = ["https://crates.io/api/v1/crates/winapi-x86_64-pc-windows-gnu/0.4.0/download"],
        strip_prefix = "winapi-x86_64-pc-windows-gnu-0.4.0",
        build_file = Label("@//third-party/bazel:BUILD.winapi-x86_64-pc-windows-gnu-0.4.0.bazel"),
    )

    return [
        struct(repo = "vendor__cc-1.0.83", is_dev_dep = False),
        struct(repo = "vendor__clap-4.4.11", is_dev_dep = False),
        struct(repo = "vendor__codespan-reporting-0.11.1", is_dev_dep = False),
        struct(repo = "vendor__once_cell-1.19.0", is_dev_dep = False),
        struct(repo = "vendor__proc-macro2-1.0.70", is_dev_dep = False),
        struct(repo = "vendor__quote-1.0.33", is_dev_dep = False),
        struct(repo = "vendor__scratch-1.0.7", is_dev_dep = False),
        struct(repo = "vendor__syn-2.0.41", is_dev_dep = False),
    ]
