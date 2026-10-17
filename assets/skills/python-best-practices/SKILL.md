---
name: python-project-standard
description: >-
  Enforce a strict, type-safe, model-agnostic, AI-navigable Python project standard:
  mypy --strict + beartype runtime type-checking + pydantic at boundaries + provider
  Protocol seams (zero-SDK core) + src layout with a real package name + domain-first
  deep structure + centralized config/logging/prompts + a one-command zero-warning gate
  + drift guards. Use this whenever starting or scaffolding a new Python project, service,
  or package; setting up pyproject/ruff/mypy/pytest; deciding where code, config, or prompts
  belong; adding type checking, runtime validation, or an LLM/embedding/vector-store
  dependency; organizing or deepening package structure; wiring providers; setting up CI or
  pre-commit; or checking that an existing Python project conforms. Apply it even when the
  user only says "start a Python project", "set up the repo", "add types", "wire up an LLM",
  or "structure this", without naming the standard.
---

# Python Project Standard

This skill is the guiding standard for **any** Python work. Apply it by default; don't wait to be asked.

Its spine: **trust is placed not in the model, but in the machine-checkable code that constrains it.** Dynamic Python's signal is too weak — code runs while doing the wrong thing. Tighten the scaffold instead: static types (mypy), runtime types (beartype), behavior (tests), a name-navigable layout, model-agnostic seams, and failing artifacts that mechanize the implicit knowledge a human would otherwise hold in their head. The agent's output ceiling equals the tightness of that feedback loop.

Baseline: **Python 3.13**, mypy `--strict`, ruff, beartype, pydantic v2 + pydantic-settings, Jinja2, Hypothesis. Modern idioms only (no `typing.List`, prefer PEP 695 `type`/`class Box[T]`).

**No `from __future__ import annotations`** — gated by ruff `TID251` banned-api. Not because it "fights beartype" (it doesn't), but because: (1) PEP 563 is an abandoned transition — 3.14 shipped PEP 649 lazy evaluation instead, keeping annotations as live objects, so the future import will never become default and solves nothing on 3.13; (2) live annotation objects are the zero-machinery path — beartype/pydantic evaluate stringified annotations through their own eval subsystems, and each layer adds edge cases (a habitual quote *plus* the future import double-stringifies and blows beartype up); (3) the rule is nearly free — the only cost is quoting forward/self references, which the standard does anyway.

## When starting a new project

```bash
python scripts/scaffold.py <package_name> --target <dir> --domains ingestion retrieval generation agents
```

This mirrors `assets/templates/` into a conforming project (src layout, `core/`, `ports/`+`adapters/`, prompts, CLAUDE.md, ADR, `ci.sh`, drift check, smoke/conformance/settings tests) and substitutes `__PACKAGE_NAME__`. Each domain package also gets a `README.md` carrying a working `covers:` frontmatter, so the drift guard has something real to protect. When adapting an existing repo, copy from `assets/templates/` by hand and replace every `__PACKAGE_NAME__` (the Python modules use absolute imports, so the name appears in their import lines too).

Then: `uv sync` once (editable install), and `./ci.sh` (or `make check`) **from the repo root** to verify. The package ships a `[project.scripts]` console script, so `uv run <package_name>` is a valid "enter through the package" entry point alongside `python -m <pkg>.cli`.

## When working on existing code

Apply the rules below, keep new code strict, and verify structural invariants:

```bash
python scripts/check_conformance.py <project_root>
```

It checks the mechanically enforceable invariants via **AST** (src layout, real package name, a real `beartype_this_package(...)` call in the package `__init__.py`, no functions defined in that top-level `__init__.py`, `core/__init__.py` empty, `settings.py` is a leaf, **no relative imports inside the package**, **a closed-set import whitelist outside `adapters/`**, the `.project-root` marker exists, and that `[tool.mypy] strict = true` is present in `pyproject.toml`). Note the last one is a **config-presence check — it does not run mypy**; the gate does that. Everything else is enforced by the gate and by applying the standard.

Scaffolding copies this script into the generated project's `scripts/` and its `ci.sh` calls it — an unchecked checker is no checker: with it outside the gate, deleting the beartype hook entirely still leaves `./ci.sh` green.

## The non-negotiables

Full rationale in `references/standard.md`.

1. **Static: mypy `--strict`, no bare `Any`.** Annotate every parameter and return; parameterize generics; PEP 695 `type` aliases; `# type: ignore` needs a code, a reason, and is tracked debt — the code part is gated by `enable_error_code = ["ignore-without-code"]`.

2. **Runtime: beartype via a central switch + claw hook — never hand-applied `@beartype`, never commented out.** Switch is `settings.beartype_on` (default **on**; only production sets `APP_BEARTYPE_ON=false`). Hook at the very top of the package `__init__.py`, which holds only the hook and re-exports — **never a function definition** (the hook installs while that file is already executing, so its own defs are never instrumented). **There is exactly one strategy: O(1) sampling** — beartype's `BeartypeStrategy.On` is still unimplemented upstream (its source says so; `On` and `O1` emit byte-identical wrappers), so any "CI runs the full O(n) check" claim is a no-op. beartype catches shape drift, not "element 7 of this list is dirty"; bad data deep inside containers is caught by pydantic at the boundary (rule 4), which validates element-wise.

3. **beartype import-ordering (top footgun).** The hook only instruments modules imported *after* it, and `sys.modules` caching makes anything loaded before it permanently unchecked. So: before the hook, import only `settings`; `core/settings.py` is a **leaf** (no first-party imports); `core/__init__.py` (and any package on the path from the top `__init__.py` to `settings`) is **empty**. This is automatic for every entry point because importing anything from the package runs `__init__.py` first — *as long as you enter through the package* (`import pkg.X`, `python -m pkg.cli`, console script, `uv run`). Never run a package module as a loose file: it loads as `__main__` *before* the hook, so its own defs stay unchecked.

4. **Boundaries: pydantic.** Every value crossing a process boundary (LLM output, tool results, HTTP, files, deserialization) is parsed and validated by a pydantic model at the entry point. Rule: crossing a process boundary → pydantic (validate + coerce); internal contract → beartype.

5. **Model-agnostic: every external AI dependency behind a `ports/` Protocol; SDKs only in `adapters/`.** Core and domain code import zero vendor SDKs. The checker enforces this as a **closed set**, not a vendor denylist: outside `adapters/`, every imported top-level package must be stdlib, the package itself, or a runtime dep declared in `[project.dependencies]`. Vendor SDKs live in `[project.optional-dependencies]`, so they are excluded automatically and no future SDK needs registering. One assembly seam (`ports/factory.py`) selects the impl by env; SDKs are lazy-imported inside adapters; vendor errors normalize to `ProviderError` (program errors propagate — never `except Exception` into a fallback). A deterministic **MockProvider is the default** (not a test stub) so the main path runs offline/CI with no SDK or key. A conformance kit binds Mock and real backends to the same invariants.

6. **No silent failures.** beartype violations, Jinja2 `StrictUndefined`, normalized provider errors — all prefer a loud, located failure over silently-wrong behavior. No bare `except`, no `except Exception` (ruff `E722` + `BLE`).

7. **Completion = one zero-warning gate, the agent's only correctness judge.** `./ci.sh` runs `ruff format --check` → `ruff check` → `mypy` → `check_conformance.py` → `check_drift.py` → `pytest` (beartype + smoke + `filterwarnings=["error"]`), `set -euo pipefail`. **Always run it from the repo root** — from a deep subdirectory pytest collects 0 tests and prints "no tests ran", which reads as "not red" while nothing was tested (`tests/conftest.py` guards this, but only when it gets loaded at all). Run it after every change; fix until green; never "looks fine, commit". Pin it with `make hooks`, which installs a git `pre-push` hook that just `exec ./ci.sh`. **No `.pre-commit-config.yaml` ships, deliberately** — a second framework means a second list of "which checks to run" drifting against `ci.sh`, and its mirrors pin their own ruff/mypy versions against the ones in `[dependency-groups]`, producing green-locally/red-in-CI. The gate must be one thing.

   Lint selection backs the prose rules with actual gates: `BLE` (no `except Exception` catch-all — rule 5's "never"), `PGH` (no blanket `type: ignore`/`noqa`), `T20` (no `print` — "library code only `getLogger`"), and `flake8-tidy-imports` `ban-relative-imports = "all"` (rule: absolute imports, now caught by ruff as well as the conformance checker, and in `tests/`/`scripts/` too). The single `T20` exemption is `per-file-ignores` for `scripts/*.py` — the gate checkers are a CLI layer whose output contract *is* stdout. That is a directory-scoped config decision, not a scattering of `noqa` comments; `src/` (including `cli.py`, which logs instead) gets no exemption.

## Structure: src layout, named package, no rootutils

- **Always src layout** (`src/<name>/`) with a **real package name** from day one (never `import src`). **No `rootutils`, no `sys.path` edits** — install editable (`uv sync`) and the `.pth` puts `ROOT/src` on the path so **import resolution** works from any CWD. **Running** still requires being inside the project tree (or setting `APP_ROOT_DIR`), because `__init__.py` executes `_find_project_root()` and raises `RuntimeError` outside it — that is the `.project-root` anchor working as designed, not a defect.
- **Finding files is a separate problem from importing.** Package-shipped resources → `importlib.resources`/`PackageLoader`; root-level content (configs/data) → settings + env. The root anchor is `ROOT_DIR` in `core/settings.py` (import it, never recompute it), resolved by walking up **from `Path.cwd()`** (cwd itself included) for a **`.project-root`** marker file at the repo root. That marker is not rootutils returning — it is an inert file checked for *existence only* with `is_file()` (its one line of "do not delete" text is never read), unrelated to import resolution; it beats `pyproject.toml` because in a monorepo every subpackage has one and the upward walk would stop at the *package* root, not the *repo* root. **Not found → `RuntimeError`, never a silent fallback to `Path.cwd()`** (that would make `ROOT_DIR` "wherever you happened to be"). Escape hatch: `APP_ROOT_DIR` — required when running outside the project tree (wheel/container). It must point at an **existing directory** or startup raises (an unvalidated bad path silently degrades every setting to its default and scatters `data/`/`logs/`), but it is deliberately **not** required to contain `.project-root` — wheels have no marker file, which is the whole point of the hatch. It is also deliberately **not** a `Settings` field, since `ROOT_DIR` is needed before `Settings()` is constructed, so `settings.py` reads it with `os.getenv` directly. The marker lives at the repo root and so never ships in the wheel; that is intended.
- **Absolute imports inside the package** (`from pkg.core.settings import get_settings`); relative imports (`from .x`, `from ..x`) are banned and checked by both the conformance checker and ruff `TID252`. A relative import dies the moment a package file is run loose (`python src/pkg/cli.py` — PyCharm's default right-click Run); the absolute form survives both that and `python -m pkg.cli`, and it carries its own coordinates so every import says where the symbol lives. Still enter through the package (`python -m pkg.cli`) — see rule 3.
- **All reusable logic in `src/<pkg>/`**, even if only `scripts/`/`tests/` call it (maximizes beartype coverage). Test-only scaffolding stays in `tests/`.
- **`core/`** = cross-cutting infra: `settings.py` (pydantic-settings, leaf; also the single source of `ROOT_DIR`/`DATA_DIR`/`LOG_DIR` and `resource_path()`), `logging.py` (`setup_logging()` once at the entry; library code only `getLogger`; `log_provenance()` + `SENSITIVE_FIELDS` keep payloads out of logs), `prompts.py`.
- **Config has two exits — don't mix them: `get_settings()` for runtime, `settings` for import time.** Exporting only an import-time module-level singleton is a silent testability break: the instance is frozen on first import, so `monkeypatch.setenv("APP_LLM_PROVIDER", ...)` changes nothing and **every provider/config branch except the default becomes untestable** — while coverage still looks fine, because those lines do execute, just always down one branch. So runtime readers (`ports/factory.py`, `core/logging.py`) call `get_settings()` (an `@lru_cache(maxsize=1)` accessor); the `settings` singleton stays only for the `__init__.py` hook, which reads `beartype_on` during import. Tests use the `override_settings` fixture in `tests/conftest.py` (set env → `get_settings.cache_clear()`), exercising the real env→`Settings` chain rather than a production injection point; `tests/test_settings.py` is its acceptance case.
- **`ports/` + `adapters/`** = the model-agnostic seam (rule 5).
- **`configs/settings.yaml`** merged into `Settings` via `YamlConfigSettingsSource` (typed replacement for Hydra config files). **`prompts/` inside the package**, loaded by `PackageLoader`, shipped in the wheel.

## Navigability: domain-first, names map to locations

Naming-as-path is to navigation what types are to interface contracts. "Fix the reranker" should resolve to `retrieval/reranking/` with no search.

- **Domain-first, not layer-first** (group by capability, not by `models/`/`utils/`). Nest 3–4 levels; leaf modules have real content. A domain package holds `models.py` (its pydantic contracts) + impl + a thin-re-exporting `__init__.py` (post-hook, so re-export is fine and aids navigation). Cross-domain contracts → top-level `schemas/`. See `references/standard.md` §7.9.

## Principles for AI-touching code (advisory)

Beyond types — for any code where a model produces output. These are upper-level discipline; not all mechanically checkable.

- **Constrain, don't ask.** Push non-negotiable properties (no fabrication, must-cite, no privilege escalation) into deterministic control flow so the model *physically cannot* violate them — don't rely on the prompt. Synthesize answers from structured values in code; discard model prose on the critical path.
- **Narrow the emission surface.** Don't let the model freely generate key payloads — make it pick from controlled options or call tools that return tri-state (found/not_found/unrecognized); take final numbers from tool results, not model text.
- **Guardrails are deterministic, independent, never pluggable.** Intent parsing can be swapped; safety decisions are deterministic code re-evaluated from raw input, not trusting a pluggable component's output.

## Driving AI on big work (advisory)

Treat AI as supervisable labor, not unsupervised autopilot. (A heavier version is arguably a sibling skill.)

- **Decision-first: write a numbered, immutable ADR (`docs/adr/`) before coding** — context + chosen option + rejected alternatives and why. AI fills within locked boundaries; rejected-reasons stop it re-walking excluded paths.
- **TDD red-light first; tests are the immutable spec.** Write the failing test, then implement to green; never weaken a test to pass. Attach the failing test as a subagent's acceptance criteria.
- **Numbered steps, each independently green; one commit per step through the full gate.** No giant diffs.
- **Adversarial independent review.** After writing, run a separate, hostile, multi-perspective review ("assume it's wrong; falsify it"), prioritizing artifacts tests can't cover (diagrams, docs, tradeoffs) — the same agent that writes and praises confirms its own bias.

## Scale to project size

The model-agnostic layer, ADRs, and drift guards are real overhead — overkill for a 200-line tool. Present them as **triggered, scalable patterns** ("the moment you call an LLM/embedding/vector store, put it behind a Protocol"; "drill a package down the moment it takes a second responsibility"), not blanket mandates.

## Resources

- `references/standard.md` — the full standard with rationale, plus an appendix carrying the **core infrastructure modules verbatim**: `pyproject.toml`, the package `__init__.py`, `core/__init__.py`, `core/settings.py`, `core/logging.py`, `core/prompts.py`, `configs/settings.yaml`, `Makefile`, `ci.sh`. (Not every module — `ports/`, `adapters/`, `cli.py` and the tests live only in `assets/templates/`.) Read it for the why, the edge cases, or exact module contents.
- `assets/templates/` — exact boilerplate, mirroring a project layout; `__PACKAGE_NAME__` is the only placeholder. Includes `ports/`+`adapters/` (model-agnostic seam), `ci.sh`, `Makefile`, `CLAUDE.md`, `docs/adr/`, `scripts/check_drift.py`, and smoke/conformance/settings tests.
- `scripts/scaffold.py` — generate a conforming new project.
- `scripts/check_conformance.py` — verify structural invariants (AST-based; incl. the closed-set import whitelist outside `adapters/`). Source of truth here; `scaffold.py` copies it into each generated project's `scripts/`, where that project's `ci.sh` runs it.
- `scripts/check_appendix_sync.py` — guards the appendix above against drift: every appendix code block must be **byte-identical** to its `assets/templates/` counterpart (normalizing `__PACKAGE_NAME__` ↔ `myproj`). This drift turns no test red — it just makes everyone reading the doc write against code that no longer exists — and it has already happened twice here, so it is mechanized rather than remembered.
- `ci.sh` — this skill's own gate: run `./ci.sh` in the skill directory to hold `scripts/` to the very rules this standard prescribes (`ruff format --check` + `ruff check` + `mypy --strict`) and to run the appendix-sync check, reading the rules straight out of `assets/templates/pyproject.toml` so there is no second config to drift. Tools come from `uvx`, so no preinstall and no accidental global/Anaconda toolchain. Run it after touching any of those scripts **or the appendix**.
