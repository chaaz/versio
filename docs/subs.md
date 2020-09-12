# Major Subdirectories

Some projects keep a separate directory for each major release after the
first leading to a layout that looks like this example:

```
project
├─ (files for v0, v1)
├─ v2
│  └─ (files for v2)
├─ v3
│  └─ (files for v3)
└─ ...
```

Using a "subs" property for a project in `.versio.yaml` causes Versio to
search for major versions in `vN`-named subdirectories. The simplest
example is an empty map value:

```yaml
- name: project
  root: "proj_main"
  subs: {}
  ...
```

While the `root` property is never required (it defaults to ".", the
repository working directory), it is especially useful to have on "subs"
projects, since it is also the directory where subdirectories are
searched for. (Conveniently, `root` is also the base directory for other
project properties, such as `changelog`, `version` files, and
`includes`/`excludes`.)

This uses the default configuration, where subdirectories are named
"v&lt;&gt;", and the top-level project is expected to hold the *0* and
*1* major versions; this default roughly corresponds to how many Golang
projects are structured. However, you can can specify other options
yourself:

```yaml
- name: project
  ...
  subs:
    dirs: "version_<>"
    tops: [0]
```

In the above, the top-level directory is expected only to hold the major
versions starting with *0*, and version subdirectories have a
"version\_&lt;&gt;" pattern (presumably starting with "version\_1"). In
the "dirs" sub-property, a single "&lt;&gt;" widget is a placeholder for
the major number.

Note that Versio will not actually move around your code into the
various subdirectories; it's expected that you still do that yourself.
However, Versio's command will error instead of assigning a version
number where it doesn't belong. For example, a major commit that would
upgrade a version number from "2.1.3" to "3.0.0" would cause the next
`versio release` to fail if that commit included files in a `v2`
subdirectory. Additionally, the `versio plan` output will warn of all
such illegal version changes. If you see such warnings, you probably
need to restructure your commits to create a new subdirectory.
