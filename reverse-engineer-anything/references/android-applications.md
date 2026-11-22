# Android application artifacts

Use the connected server's advertised Android tools. Repository main and npm
4.1.0 include this family. A skill installation alone cannot add it to an
older server.

Start with `inspect_android_package` on the caller's APK path for package
identity, manifest declarations, coverage, and inline Evidence. Use
`search_android_classes` to find candidates, `inspect_android_class` for one
class's members, `inspect_android_method` for decompilation, and
`trace_android_references` for incoming static references. Each call takes its
own APK path and explicit selectors; do not open a native session first.
Choose an `overload_index` from the class inventory when methods are ambiguous
instead of guessing.

For JNI implementations, discover `inspect_android_jni_bindings`. Select raw
DEX class/method identities and optional descriptors, ABI or logical library
paths. Prefer `detail: "summary"`; complete Evidence remains retained and
`android-jni` views page declarations, libraries, candidates or binding rows
with exact fields. Global native-pair ambiguity survives class selection.
Preserve slot/relocation provenance and source coverage: matched means observed
candidate presence; partial scans do not establish uniqueness or absence.
This reads DEX/ELF bytes without loading the APK into the decompiler or executing
target code. Runtime registration and loading remain unknown. See the
[JNI guide](https://github.com/morluto/rea/blob/main/docs/guides/android/android-analysis.md#link-native-declarations-to-jni-candidates).

For an R8/ProGuard release build with its `mapping.txt`, pass the absolute
`mapping_path` to `inspect_android_class` or `inspect_android_method`. Prefer
`detail: "summary"` to receive normalized symbols or the selected body without
duplicate raw class metadata. Complete Evidence stays retained under the linked
parent ID and can be exported; CLI summary delivery requires `--evidence_output`.
Omitted or complete detail preserves the full inline response. Keep
selecting by observed names; the separate `mapping` block reports each symbol
as `mapped`, `ambiguous` or `unmapped` after checking DEX types and prototypes.
REA does not verify that the mapping belongs to the APK build.

The provider requires a separately supplied headless JADX MCP JAR 0.7.1 and
Java 17 or newer. `REA_JADX_MCP_JAR` selects the absolute JAR path; `JAVA_HOME`
can select Java. Use `rea doctor --provider jadx --json` to check the JADX
configuration and obtain remediation for missing prerequisites. Java and the
headless JADX MCP JAR remain caller-supplied; REA setup does not install or
configure them. Configuration readiness does not establish successful analysis
of a particular APK.

Linux and macOS use the POSIX owned-process boundary. Windows x64 requires the
matching bundled native controls and uses an owned Job Object with bidirectional
stdio. A full JDK, including `jdk.compiler`, is required on every host; Windows
`JAVA_HOME` selects `bin\java.exe`. Consult the Android guide for actual host
verification coverage. Calls use one worker and provider deadlines; optional
`REA_JADX_HEAP_MIB` selects the JVM heap and `REA_JADX_OPERATION_TIMEOUT_MS`
the per-operation deadline (integer milliseconds from 1 to 2147483647, default
120000). To change that deadline on a running MCP server, set
`jadx.operation_timeout_ms` through `update_analysis_settings`, then retry after
work and cleanup settle. Newly accepted operations use the override; accepted
work keeps its budget. Changing the launch environment requires a new process.
The MCP client's tool timeout must also allow the selected deadline. The APK is never launched,
and no emulator is required.

Manifest entries and decompiled text are static observations, not execution or
policy enforcement. Incoming references are recovered static relationships;
reflection, dynamic loading, and unresolved calls remain unknown. Preserve
artifact digests, locations, diagnostics, and incomplete coverage. For archive
members rather than code analysis, use `open_binary` and `inspect_artifact`.
