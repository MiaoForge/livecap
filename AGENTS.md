## Documentation Language

- Write documentation in English by default.
- Chinese documentation must use a `-zh` or `-cn` filename suffix before the extension, for example `README-zh.md` or `setup-cn.md`.
- Prefer `-zh` for new Simplified Chinese documents. Keep release notes in English at `docs/release/releasenote-<tag>.md`.
- Keep current user and maintainer guides in `docs/`. Put historical prototype and research material in `docs/archive/`.

## Commit Conventions

- Use Conventional Commits: `<type>(<scope>): <description>`. The scope is optional.
- Common types are `feat`, `fix`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `perf`, and `style`.
- Write commit messages in English. Keep the description concise and specific, for example `docs: clarify contributor guidelines` or `fix(translation-core): discard stale translation results`.
- Give each commit one clear purpose. Do not mix unrelated refactoring, formatting, or temporary files into the same commit.
- For larger changes, commit at appropriate milestones as cohesive, verified pieces of work are completed. Keep individual commits reasonably small and easy to review instead of accumulating one oversized commit.
- Mark breaking changes with `!` and explain their impact and migration steps in a `BREAKING CHANGE:` footer.
- Review the staged diff before committing. Exclude secrets, credentials, debug artifacts, and unintended changes.

## Pre-commit Verification

- Lint and formatting checks must pass before committing. Use the repository scripts; editor formatting alone is insufficient.
- Run tests for the changed behavior and affected consumers. Changes to shared code or protocols require checks of the relevant consumers, not just the edited files.
- Add regression tests for bug fixes and update tests for behavior changes. Documentation-only changes do not require code tests, but Markdown formatting, links, and file references must be checked.
- Changes to types, build configuration, or dependencies also require type checks and builds for the affected components.
- Fix failures and rerun the checks. Do not bypass checks, disable rules, or delete valid tests to make a change pass.
- Run `npm run check` before committing. It runs lint, formatting checks, type checks, tests, and configured builds. Use `npm test -- <file-path>` to target relevant tests. The initial scaffold may have no tests; business logic must have meaningful tests, and the absence of tests is not evidence that behavior is correct.
- Report the checks performed and their results. Explain any checks that were not run.

## Architecture and Code Quality

- Keep the architecture clear: each module has one responsibility, dependencies have an explicit direction, and modules interact through public interfaces.
- Keep transcript and language logic independent of browser and Windows host APIs. Keep audio capture, credentials, and persistence in the native layer.
- Keep coupling low. Avoid circular dependencies, access to another layer's internals, and shared mutable global state connecting modules.
- Prefer simple, direct, readable implementations. Introduce abstractions to remove actual duplication or isolate a clear boundary, not to anticipate hypothetical needs.
- Remove redundant code, dead branches, temporary debugging logic, and duplicate implementations. Compatibility code must have a clear purpose and supported scope.
- Use names that express domain meaning and define clear types and error boundaries. Comments should explain necessary constraints and tradeoffs rather than repeat the code.
- Keep changes focused. Refactor only as needed for the current task, preserve verifiable behavior, and avoid rewriting unrelated modules.
- Test observable behavior, edge cases, and failure paths. Avoid tests that merely mirror implementation details or add little value for low-impact changes.
