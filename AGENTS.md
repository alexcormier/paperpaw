# For AI assistants

There are two kinds of work here; tell which one a task is.

- **Work on QuireInk itself**: its kernel, SDK, shells and firmware, and
  its own plugins in `plugins/`. Follow [CONTRIBUTING.md](CONTRIBUTING.md):
  its architecture rules and the checks to run. Put a change at the
  layer where it belongs; when a plugin cannot do something well, extend
  the SDK (with its API file and CHANGELOG brought up to date) rather
  than working around it in the plugin.
- **A plugin for QuireInk as it is**, written outside the project without
  changing QuireInk (e.g. a community plugin in its own repository):
  follow the skill in
  [`.claude/skills/quireink-plugin-development`](.claude/skills/quireink-plugin-development/SKILL.md),
  which builds on the SDK as released. Its limits do not apply to work
  on QuireInk itself.

[docs/plugins.md](docs/plugins.md) explains how plugins are written, for
both.
