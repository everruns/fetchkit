## Coding-agent guidance (repo root)

This repo is intended to be runnable locally and easy for coding agents to work in.

Style
Telegraph. Drop filler/grammar. Min tokens (global AGENTS + replies).

Critical Thinking
Fix root cause (not band-aid). Unsure: read more code; if still stuck, ask w/ short options. Unrecognized changes: assume other agent; keep going; focus your changes. If it causes issues, stop + ask user. Leave breadcrumb notes in thread.

### Principles

- Keep decisions as comments on top of the file. Only important decisions that could not be inferred from code.
- Code should be easily testable, smoke testable, runnable in local dev env.
- Prefer small, incremental PR-sized changes with a runnable state at each step.
- Avoid adding dependencies with non-permissive licenses. If a dependency is non-permissive or unclear, stop and ask the repo owner.

### Top level requirements

< insert top level requirements >

### Specs

`specs/` folder contains feature specifications outlining requirements for specific features and components. New code should comply with these specifications or propose changes to them.

Avaible specs:
- <`spec name.md` - Spec description>

Specification format: Abstract and Requirements sections.

### Skills

`.claude/skills/` contains development skills following the [Agent Skills Specification](https://agentskills.io/specification).

Available skills:
- <`.claude/skills/skill-name.md` - Skill description>


### Public Documentation

`docs/` contains public-facing user documentation. This documentation is intended for end users and operators of the system, not for internal development reference.


When making changes that affect user-facing behavior or operations, update the relevant docs in this folder.

### Local dev expectations

< insert requirements for locel dev env >


### Code organization

< insert code organisation >

### Naming

< insert naming conventions >


### CI expectations

- CI is implemented using GitHub Actions.
< insert other CI expectation >

### Pre-PR checklist

Before creating a pull request, ensure:

1. **Formatting**: Run ... to format all code
2. **Linting**: Run ... and fix all warnings
3. **Tests**: Run ... to ensure all tests pass
4. **Smoke tests**: Run smoke tests to verify the system works end-to-end
5. **Specs**: If your changes affect system behavior, update the relevant specs in `specs/`. Check that changes are not conflicting with specs
6. **Docs**: If your changes affect usage or configuration, update public docs in `./docs` folder

CI will fail if formatting, linting, tests, or UI build fail. Always run these locally before pushing.

### Commit message conventions

Follow [Conventional Commits](https://www.conventionalcommits.org) for all commit messages:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style (formatting, semicolons, etc.)
- `refactor`: Code refactoring without feature/fix
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Build process, dependencies, tooling
- `ci`: CI configuration changes

**Examples:**
```
feat(api): add agent versioning endpoint
fix(workflow): handle timeout in run execution
docs: update API documentation
refactor(db): simplify connection pooling
```

**Validation (optional):**
```bash
# Validate a commit message
echo "feat: add new feature" | npx commitlint

# Validate last commit
npx commitlint --from HEAD~1 --to HEAD
```

### PR (Pull Request) conventions

PR titles should follow Conventional Commits format. Use the PR template (`.github/pull_request_template.md`) for descriptions.

**PR Body Template:**

```markdown
## What
Clear description of the change.

## Why
Problem or motivation.

## How
High-level approach.

## Risk
- Low / Medium / High
- What can break

### Checklist
- [ ] Unit tests are passed
- [ ] Smoke tests are passed
- [ ] Documenation is updated
- [ ] Specs are up to date and not in conflict
- [ ] ... other check list items
```

### Testing the system

< insert isntructions on testing the system >
