# Publishing

Creating a new version of your software isn't useful if no-one can get
it. That's why Versio can also publish your software to all kinds of
targets.

For some deploys, you'll need the specific build tools such as
compilers; others might require the package manager (`npm`, `mvn`,
etc.). Some publishing might require authorization, so be sure you're
logged in, or have your credentials handy.

## Common deploys

Many package managers have a central repository that gives access to all
the world. Others allow publications to organisation-level or local
repositories for more limited distribution.

### NPM (Node.js projects)

### Maven (Java projects)

### Crates.io (Rust projects)

### proxy.golang.org (Go projects)

### PyPI (Python)

### RubyGems.org (Ruby)

### Dockerhub (Docker)

### GitHub Releases

A GitHub release is a general-purpose distribution, allowing users to
download source code and/or binary products directly. Although it's not
as suited to direct use by package managers, it's a good spot for users
to download independent resources.

> TODO : single GitHub release for multiple projects

## Custom Docker

One of the advantages of having a monorepo is being able to put both the
source code and the deployment descriptors in different projects. Versio
can take advantage of that: you can deploy your source project using a
separate Docker project.

> TODO: example

## Inline Docker options

There are common patterns for creating docker images. If you planning on
using one of them, you don't need a separate Docker project: just (TODO:
what actually would you do here?)

> TODO: example

If you ever want to customize, just use (TODO: something easy), and
Versio will write the files it uses to a custom project; from there on,
you can customize as you like and deploy from that project.

> TODO: example

### Docker: Rust native cmdline

### Docker: Go native cmdline

### Docker: Java 13 cmdline (Java main)

### Docker: Node.js 12 cmdline (index.js)

### Docker: Ruby 2.7 cmdline (index.rb)

### Docker: Python 3.8 cmdline (\_\_name\_\_)

### Docker: Java webapp (Tomcat 10, Java 12)

### Docker: Node.js webapp (express ?)

### Docker: Ruby 2.7 webapp (Rails ?)

### Docker: Python 3.8 webapp (?)

## Inline webapp options

If you're writing a webapp, you don't need to host it yourself: there
are some common ways to publish a webapp without needing to manage your
own public infrastructure.

> TODO: is this even true?

If you ever want to customize, just use (TODO: something easy), and
Versio will write the files it uses to a custom project.

### Tomcat Java hosting

### Express Node.js hosting

### etc

## Custom Helm

If you're planning to run your software on a Kubernetes cluster, then
Helm / Helmfile is one way to do this. Create a custom helm deployment
in a project that references your docker images (in another project),
and then use `versio deploy` to send everything up.

> TODO: example
