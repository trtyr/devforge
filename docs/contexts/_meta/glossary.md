# 词表（由 inventory 自动导出，勿手改）

## unit-src-01

- `BoundaryVerdict` — `src/change/boundary.rs:25`
- `ImpactGap` — `src/change/boundary.rs:17`
- `MAX_ROUNDTRIPS` — `src/change/boundary.rs:13`
- `check_roundtrip` — `src/change/boundary.rs:159`
- `classify` — `src/change/boundary.rs:133`
- `is_fail` — `src/change/boundary.rs:35`
- `merge_gaps_into_whitelist` — `src/change/boundary.rs:169`
- `parse_impact_gaps` — `src/change/boundary.rs:79`
- `parse_route` — `src/change/boundary.rs:45`
- `replan_instruction` — `src/change/boundary.rs:180`

## unit-src-02

- `assign_change_id` — `src/change/change_id.rs:55`
- `change_root` — `src/change/change_id.rs:15`
- `current_marker` — `src/change/change_id.rs:20`
- `date_segment` — `src/change/change_id.rs:87`
- `date_segment_now` — `src/change/change_id.rs:94`
- `make_change_id` — `src/change/change_id.rs:48`
- `slugify` — `src/change/change_id.rs:25`
- `validate_change_id` — `src/change/change_id.rs:132`

## unit-src-04

- `BASELINE_LOG` — `src/change/coding_gate.rs:16`
- `BaselineEvidence` — `src/change/coding_gate.rs:26`
- `COMMIT_PREFIX` — `src/change/coding_gate.rs:22`
- `IMPL_STEPS` — `src/change/coding_gate.rs:20`
- `SELF_CHECK_LOG` — `src/change/coding_gate.rs:18`
- `Step` — `src/change/coding_gate.rs:160`
- `changed_files` — `src/change/coding_gate.rs:210`
- `check_baseline_log` — `src/change/coding_gate.rs:109`
- `check_self_check` — `src/change/coding_gate.rs:135`
- `check_step_commits` — `src/change/coding_gate.rs:283`
- `commits_since` — `src/change/coding_gate.rs:231`
- `impl_steps_path` — `src/change/coding_gate.rs:323`
- `parse_baseline_log` — `src/change/coding_gate.rs:104`
- `parse_steps` — `src/change/coding_gate.rs:165`
- `read_steps` — `src/change/coding_gate.rs:191`
- `step_numbers_in` — `src/change/coding_gate.rs:256`

## unit-src-07

- `DocAction` — `src/change/docs.rs:17`
- `apply` — `src/change/docs.rs:31`
- `essential_ready` — `src/change/docs.rs:52`
- `inventory_path` — `src/change/docs.rs:56`

## unit-src-08

- `Baseline` — `src/change/gate0.rs:17`
- `DocLevel` — `src/change/gate0.rs:24`
- `append_roundtrip` — `src/change/history.rs:17`
- `as_str` — `src/change/gate0.rs:45`
- `ensure_baseline` — `src/change/gate0.rs:66`
- `history_path` — `src/change/history.rs:10`
- `needs_docs` — `src/change/gate0.rs:54`
- `parse` — `src/change/gate0.rs:34`

## unit-src-09

- `ChangeOutcome` — `src/change/orchestrator.rs:149`
- `ChangeRun` — `src/change/orchestrator.rs:130`
- `ChangeSession` — `src/change/orchestrator.rs:88`
- `CodingTurn` — `src/change/orchestrator.rs:63`
- `MAX_PLAN_FIXES` — `src/change/orchestrator.rs:24`
- `MAX_TEST_REROUTES` — `src/change/orchestrator.rs:21`
- `PlanTurn` — `src/change/orchestrator.rs:56`
- `TestingTurn` — `src/change/orchestrator.rs:74`
- `boundary` — `src/change/mod.rs:10`
- `change_id` — `src/change/mod.rs:11`
- `coding_gate` — `src/change/mod.rs:12`
- `docs` — `src/change/mod.rs:13`
- `gate0` — `src/change/mod.rs:14`
- `history` — `src/change/mod.rs:15`
- `orchestrator` — `src/change/mod.rs:16`
- `plan_audit` — `src/change/mod.rs:17`
- `prepare` — `src/change/mod.rs:18`
- `report` — `src/change/mod.rs:19`
- `run` — `src/change/orchestrator.rs:155`
- `session` — `src/change/mod.rs:20`
- `state` — `src/change/mod.rs:21`
- `testing_gate` — `src/change/mod.rs:22`

## unit-src-12

- `PATH_MIGRATIONS` — `src/change/plan_audit.rs:28`
- `PLAN_BLOCKS` — `src/change/plan_audit.rs:13`
- `PlanAudit` — `src/change/plan_audit.rs:226`
- `VERDICT_ANSWER_FIELD` — `src/change/plan_audit.rs:61`
- `VERDICT_MUST_ASK` — `src/change/plan_audit.rs:55`
- `VERDICT_SELF` — `src/change/plan_audit.rs:58`
- `VerdictAudit` — `src/change/plan_audit.rs:65`
- `audit` — `src/change/plan_audit.rs:261`
- `audit_plan_file` — `src/change/plan_audit.rs:442`
- `audit_verdicts` — `src/change/plan_audit.rs:98`
- `is_clean` — `src/change/plan_audit.rs:73`
- `is_clean` — `src/change/plan_audit.rs:236`
- `normalize_path` — `src/change/plan_audit.rs:39`
- `path_refs` — `src/change/plan_audit.rs:318`
- `plan_facts` — `src/change/plan_audit.rs:387`
- `render` — `src/change/plan_audit.rs:241`
- `render_facts` — `src/change/plan_audit.rs:78`
- `review_instruction` — `src/change/plan_audit.rs:409`

## unit-src-15

- `ChangePrep` — `src/change/prepare.rs:15`
- `prepare` — `src/change/prepare.rs:30`

## unit-src-16

- `AcceptanceSummary` — `src/change/report.rs:21`
- `ChangeReport` — `src/change/report.rs:35`
- `DiffStats` — `src/change/report.rs:13`
- `diff_stats` — `src/change/report.rs:95`
- `passed` — `src/change/report.rs:28`
- `render` — `src/change/report.rs:49`
- `summarize_acceptance` — `src/change/report.rs:125`
- `write_report` — `src/change/report.rs:139`

## unit-src-17

- `ChangeCtx` — `src/change/session.rs:30`
- `ISSUES_NUDGE` — `src/change/session.rs:175`
- `MAX_EMPTY_ISSUES_NUDGES` — `src/change/session.rs:172`
- `ProdChangeSession` — `src/change/session.rs:55`
- `acceptance_path` — `src/change/session.rs:49`
- `baseline_kickoff` — `src/change/session.rs:89`
- `baseline_log_path` — `src/change/session.rs:584`
- `coding_kickoff` — `src/change/session.rs:146`
- `coding_review_instruction` — `src/change/session.rs:103`
- `plan_exists` — `src/change/session.rs:579`
- `plan_kickoff` — `src/change/session.rs:69`
- `plan_path` — `src/change/session.rs:45`
- `testing_kickoff` — `src/change/session.rs:179`
- `testing_review_instruction` — `src/change/session.rs:125`

## unit-src-21

- `CHANGE_SOP` — `src/change/state.rs:15`
- `ChangePhase` — `src/change/state.rs:20`
- `ChangeState` — `src/change/state.rs:53`
- `TestFailureContext` — `src/change/state.rs:44`
- `as_str` — `src/change/state.rs:28`
- `bump_review` — `src/change/state.rs:133`
- `bump_roundtrip` — `src/change/state.rs:137`
- `bump_test_reroute` — `src/change/state.rs:141`
- `clear_test_failure` — `src/change/state.rs:154`
- `from_run` — `src/change/state.rs:99`
- `load` — `src/change/state.rs:160`
- `mark_acceptance_done` — `src/change/state.rs:127`
- `mark_step_done` — `src/change/state.rs:120`
- `new` — `src/change/state.rs:84`
- `save` — `src/change/state.rs:168`
- `set_test_failure` — `src/change/state.rs:149`
- `write_into` — `src/change/state.rs:108`

## unit-src-23

- `AcceptanceMatch` — `src/change/testing_gate.rs:22`
- `FAILURE_BLOCK_TITLE` — `src/change/testing_gate.rs:137`
- `REGRESSION_LOG` — `src/change/testing_gate.rs:18`
- `check_regression_claim` — `src/change/testing_gate.rs:28`
- `check_regression_log` — `src/change/testing_gate.rs:45`
- `coding_failure_block` — `src/change/testing_gate.rs:142`
- `failure_details` — `src/change/testing_gate.rs:128`
- `parse_acceptance_table` — `src/change/testing_gate.rs:57`
- `reconcile_acceptance` — `src/change/testing_gate.rs:77`
- `route_back_message` — `src/change/testing_gate.rs:118`

## unit-src-24

- `FakeInstaller` — `src/config/installer.rs:71`
- `PackageInstaller` — `src/config/installer.rs:47`
- `PiInstallBridge` — `src/config/installer.rs:52`
- `build_install_command` — `src/config/installer.rs:12`

## unit-src-25

- `AgentManifest` — `src/config/manifest.rs:122`
- `BASE_PACKAGES` — `src/config/manifest.rs:14`
- `CHANGE_CODING_AGENT_PROMPT` — `src/config/manifest.rs:186`
- `CHANGE_CODING_REVIEW_PROMPT` — `src/config/manifest.rs:189`
- `CHANGE_PLAN_AGENT_PROMPT` — `src/config/manifest.rs:180`
- `CHANGE_PLAN_REVIEW_PROMPT` — `src/config/manifest.rs:183`
- `CHANGE_TEST_AGENT_PROMPT` — `src/config/manifest.rs:192`
- `CHANGE_TEST_REVIEW_PROMPT` — `src/config/manifest.rs:195`
- `CODING_AGENT_EXTRA` — `src/config/manifest.rs:25`
- `CODING_AGENT_PROMPT` — `src/config/manifest.rs:74`
- `CODING_REVIEW_EXTRA` — `src/config/manifest.rs:32`
- `CODING_REVIEW_PROMPT` — `src/config/manifest.rs:77`
- `ConfigManifest` — `src/config/manifest.rs:166`
- `DOC_AGENT_PROMPT` — `src/config/manifest.rs:89`
- `DOC_FIX_PROMPT` — `src/config/manifest.rs:86`
- `DOC_REVIEW_PROMPT` — `src/config/manifest.rs:92`
- `McpServer` — `src/config/manifest.rs:42`
- `ModelEntry` — `src/config/manifest.rs:96`
- `PLAN_AGENT_EXTRA` — `src/config/manifest.rs:22`
- `PathPermissions` — `src/config/manifest.rs:149`
- `ProviderDef` — `src/config/manifest.rs:111`
- `TEST_AGENT_EXTRA` — `src/config/manifest.rs:35`
- `TEST_AGENT_PROMPT` — `src/config/manifest.rs:80`
- `TEST_REVIEW_PROMPT` — `src/config/manifest.rs:83`
- `builtin` — `src/config/manifest.rs:200`
- `codegraph_mcp` — `src/config/manifest.rs:54`
- `context7_mcp` — `src/config/manifest.rs:64`
- `get_agent` — `src/config/manifest.rs:497`
- `load` — `src/config/manifest.rs:512`
- `resolve_model` — `src/config/manifest.rs:474`
- `resolve_provider` — `src/config/manifest.rs:487`
- `save` — `src/config/manifest.rs:504`

## unit-src-27

- `Renderer` — `src/config/render.rs:29`
- `installer` — `src/config/mod.rs:6`
- `manifest` — `src/config/mod.rs:7`
- `render` — `src/config/mod.rs:8`
- `render_agent` — `src/config/render.rs:38`
- `render_all` — `src/config/render.rs:257`
- `skill_assets` — `src/config/mod.rs:9`

## unit-src-30

- `CODING_DISCIPLINE_SKILL_MD` — `src/config/skill_assets.rs:13`
- `DOC_CONTRACT_SKILL_MD` — `src/config/skill_assets.rs:10`
- `PLAN_TREE_SKILL_MD` — `src/config/skill_assets.rs:7`
- `builtin_language_files` — `src/config/skill_assets.rs:74`
- `builtin_skill_files` — `src/config/skill_assets.rs:18`

## unit-src-31

- `CoverageMiss` — `src/docgen/audit.rs:53`
- `CoverageReport` — `src/docgen/audit.rs:63`
- `DocLevel` — `src/docgen/audit.rs:14`
- `coverage` — `src/docgen/audit.rs:113`
- `coverage_for_unit` — `src/docgen/audit.rs:138`
- `duplicate_mentions` — `src/docgen/audit.rs:298`
- `duplicated_content` — `src/docgen/audit.rs:333`
- `parse` — `src/docgen/audit.rs:24`
- `passed` — `src/docgen/audit.rs:70`
- `render` — `src/docgen/audit.rs:75`
- `runbook_commands` — `src/docgen/audit.rs:260`
- `uncovered_units` — `src/docgen/audit.rs:406`

## unit-src-33

- `ASSEMBLY_PAGES` — `src/docgen/impact.rs:36`
- `ChangedFile` — `src/docgen/impact.rs:15`
- `Impact` — `src/docgen/impact.rs:26`
- `changed_since` — `src/docgen/impact.rs:133`
- `impacted` — `src/docgen/impact.rs:294`
- `parse_name_status` — `src/docgen/impact.rs:58`
- `parse_porcelain` — `src/docgen/impact.rs:94`
- `plan_impact` — `src/docgen/impact.rs:360`
- `reference_files` — `src/docgen/impact.rs:198`
- `units_touching` — `src/docgen/impact.rs:160`
- `vanished_names` — `src/docgen/impact.rs:173`

## unit-src-34

- `Closure` — `src/docgen/inventory/data.rs:108`
- `ClosureFinding` — `src/docgen/inventory/data.rs:125`
- `Coverage` — `src/docgen/inventory/data.rs:64`
- `EntryKind` — `src/docgen/inventory/data.rs:53`
- `Inventory` — `src/docgen/inventory/data.rs:102`
- `InventoryEntry` — `src/docgen/inventory/data.rs:70`
- `InventoryMeta` — `src/docgen/inventory/data.rs:92`
- `Lang` — `src/docgen/inventory/data.rs:32`
- `SOURCE_EXTENSIONS` — `src/docgen/inventory/data.rs:6`
- `as_str` — `src/docgen/inventory/data.rs:115`
- `assign_units` — `src/docgen/inventory/scanner.rs:340`
- `build_inventory` — `src/docgen/inventory/scanner.rs:6`
- `check_closures` — `src/docgen/inventory/closure.rs:6`
- `config_keys` — `src/docgen/inventory/support.rs:8`
- `entry_paths` — `src/docgen/inventory/data.rs:133`
- `env_file_keys` — `src/docgen/inventory/support.rs:26`
- `export_glossary` — `src/docgen/inventory/closure.rs:98`
- `from_path` — `src/docgen/inventory/data.rs:42`
- `is_entry_file` — `src/docgen/inventory/scanner.rs:226`
- `is_test_file` — `src/docgen/inventory/support.rs:183`
- `load` — `src/docgen/inventory/data.rs:158`
- `make_targets` — `src/docgen/inventory/support.rs:73`
- `package_scripts` — `src/docgen/inventory/support.rs:97`
- `public_symbols` — `src/docgen/inventory/scanner.rs:248`
- `resolve_imports` — `src/docgen/inventory/imports.rs:6`
- `save` — `src/docgen/inventory/data.rs:149`
- `scan_source_files` — `src/docgen/inventory/scanner.rs:178`
- `scan_support_files` — `src/docgen/inventory/support.rs:148`
- `sql_objects` — `src/docgen/inventory/support.rs:48`
- `symbol_names` — `src/docgen/inventory/data.rs:141`
- `test_specs` — `src/docgen/inventory/support.rs:202`

## unit-src-38

- `Check` — `src/docgen/lint/report.rs:7`
- `EntityIndex` — `src/docgen/lint/entities.rs:39`
- `EntryType` — `src/docgen/lint/report.rs:124`
- `LintReport` — `src/docgen/lint/report.rs:69`
- `Severity` — `src/docgen/lint/report.rs:39`
- `Violation` — `src/docgen/lint/report.rs:47`
- `alias_of` — `src/docgen/lint/entities.rs:93`
- `as_str` — `src/docgen/lint/report.rs:25`
- `build` — `src/docgen/lint/entities.rs:48`
- `failures` — `src/docgen/lint/report.rs:78`
- `has_name` — `src/docgen/lint/entities.rs:85`
- `has_path` — `src/docgen/lint/entities.rs:89`
- `lint_docs` — `src/docgen/lint/checks.rs:8`
- `lint_docs_with_entities` — `src/docgen/lint/checks.rs:13`
- `mentioned_entities` — `src/docgen/lint/entities.rs:7`
- `parse` — `src/docgen/lint/report.rs:138`
- `passed` — `src/docgen/lint/report.rs:85`
- `render` — `src/docgen/lint/report.rs:57`
- `required_sections` — `src/docgen/lint/report.rs:153`
- `summary` — `src/docgen/lint/report.rs:89`
- `violations` — `src/docgen/lint/report.rs:74`

## unit-src-42

- `Isolation` — `src/domain/contract.rs:12`
- `Usage` — `src/docgen/usage.rs:10`
- `WorkItemContract` — `src/domain/contract.rs:21`
- `audit` — `src/docgen/mod.rs:6`
- `covers` — `src/domain/contract.rs:38`
- `from_session_dirs` — `src/docgen/usage.rs:27`
- `impact` — `src/docgen/mod.rs:7`
- `inventory` — `src/docgen/mod.rs:8`
- `lint` — `src/docgen/mod.rs:9`
- `render` — `src/docgen/usage.rs:18`
- `usage` — `src/docgen/mod.rs:10`

## unit-src-43

- `Project` — `src/domain/project.rs:10`
- `Run` — `src/domain/run.rs:28`
- `RunControl` — `src/domain/runner.rs:17`
- `RunEngine` — `src/domain/runner.rs:69`
- `RunId` — `src/domain/run.rs:11`
- `RunOutcome` — `src/domain/runner.rs:76`
- `RunStatus` — `src/domain/run.rs:20`
- `Runner` — `src/domain/runner.rs:81`
- `cancel` — `src/domain/runner.rs:50`
- `contract` — `src/domain/mod.rs:8`
- `ensure_local_dir` — `src/domain/project.rs:29`
- `generate` — `src/domain/run.rs:14`
- `is_cancelled` — `src/domain/runner.rs:46`
- `is_stalled` — `src/domain/runner.rs:63`
- `mark_stalled` — `src/domain/runner.rs:59`
- `new` — `src/domain/project.rs:19`
- `new` — `src/domain/run.rs:38`
- `new` — `src/domain/runner.rs:33`
- `new` — `src/domain/runner.rs:86`
- `now_ms` — `src/domain/mod.rs:22`
- `project` — `src/domain/mod.rs:6`
- `run` — `src/domain/mod.rs:7`
- `run_task` — `src/domain/runner.rs:92`
- `runner` — `src/domain/mod.rs:9`
- `sop` — `src/domain/mod.rs:10`
- `store` — `src/domain/mod.rs:11`
- `task` — `src/domain/mod.rs:12`
- `timed_out` — `src/domain/runner.rs:54`
- `with_deadline` — `src/domain/runner.rs:41`

## unit-src-44

- `CHANGE_SOP` — `src/domain/sop.rs:46`
- `CODING_SOP` — `src/domain/sop.rs:18`
- `DOC_FIX_SOP` — `src/domain/sop.rs:32`
- `DOC_FROM_ZERO_SOP` — `src/domain/sop.rs:39`
- `FinishOutcome` — `src/domain/store.rs:27`
- `PLANNING_SOP` — `src/domain/sop.rs:11`
- `Sop` — `src/domain/sop.rs:53`
- `SopRegistry` — `src/domain/sop.rs:62`
- `Store` — `src/domain/store.rs:19`
- `TESTING_SOP` — `src/domain/sop.rs:25`
- `begin_run` — `src/domain/store.rs:153`
- `builtin` — `src/domain/sop.rs:68`
- `create_project` — `src/domain/store.rs:44`
- `create_task` — `src/domain/store.rs:67`
- `finish_run` — `src/domain/store.rs:181`
- `get` — `src/domain/sop.rs:81`
- `get_project` — `src/domain/store.rs:55`
- `get_run` — `src/domain/store.rs:216`
- `get_task` — `src/domain/store.rs:106`
- `has_running_run` — `src/domain/store.rs:148`
- `ids` — `src/domain/sop.rs:95`
- `list_projects` — `src/domain/store.rs:59`
- `list_runs` — `src/domain/store.rs:220`
- `list_tasks` — `src/domain/store.rs:119`
- `load` — `src/domain/store.rs:235`
- `new` — `src/domain/store.rs:33`
- `remove_project` — `src/domain/store.rs:40`
- `save` — `src/domain/store.rs:226`
- `set_task_sop` — `src/domain/store.rs:125`
- `validate` — `src/domain/sop.rs:86`

## unit-src-45

- `Task` — `src/domain/task.rs:21`
- `TaskStatus` — `src/domain/task.rs:7`
- `can_begin_run` — `src/domain/task.rs:64`
- `new` — `src/domain/task.rs:44`

## unit-src-46

- `BuiltinRpcEngine` — `src/engine/builtin.rs:53`
- `preflight_codegraph` — `src/engine/builtin.rs:26`
- `profiles_for_sop` — `src/engine/builtin.rs:87`

## unit-src-48

- `RpcClient` — `src/engine/client.rs:40`
- `WaitOutcome` — `src/engine/client.rs:18`
- `kill` — `src/engine/client.rs:414`
- `ledger_entry` — `src/engine/client.rs:157`
- `prompt` — `src/engine/client.rs:173`
- `require_success` — `src/engine/client.rs:402`
- `spawn` — `src/engine/client.rs:52`
- `spawn_with_command` — `src/engine/client.rs:57`
- `wait_settled` — `src/engine/client.rs:197`
- `wait_settled_with_ui` — `src/engine/client.rs:233`

## unit-src-50

- `LedgerEntry` — `src/engine/ledger.rs:13`
- `MANAGED_ENV` — `src/engine/ledger.rs:38`
- `Reconciliation` — `src/engine/ledger.rs:28`
- `SpawnLedger` — `src/engine/ledger.rs:22`
- `has_devforge_marker` — `src/engine/ledger.rs:100`
- `kill_orphan` — `src/engine/ledger.rs:105`
- `load` — `src/engine/ledger.rs:57`
- `process_alive` — `src/engine/ledger.rs:82`
- `read_managed_marker` — `src/engine/ledger.rs:87`
- `reconcile` — `src/engine/ledger.rs:65`
- `record` — `src/engine/ledger.rs:41`
- `remove` — `src/engine/ledger.rs:45`
- `save` — `src/engine/ledger.rs:49`
- `scan_and_kill_run` — `src/engine/ledger.rs:119`

## unit-src-51

- `MAX_ATTEMPTS` — `src/engine/policy.rs:14`
- `RetryPolicy` — `src/engine/policy.rs:23`
- `builtin` — `src/engine/mod.rs:9`
- `client` — `src/engine/mod.rs:8`
- `default_backoff_ms` — `src/engine/policy.rs:17`
- `fast` — `src/engine/policy.rs:42`
- `is_infra_failure` — `src/engine/policy.rs:60`
- `ledger` — `src/engine/mod.rs:10`
- `map_intervention` — `src/engine/policy.rs:194`
- `policy` — `src/engine/mod.rs:13`
- `process` — `src/engine/mod.rs:11`
- `protocol` — `src/engine/mod.rs:12`
- `run_attempt_bookkeeping` — `src/engine/policy.rs:237`
- `run_with_retry` — `src/engine/policy.rs:68`

## unit-src-52

- `SpawnSpec` — `src/engine/process.rs:13`
- `build_spawn_command` — `src/engine/process.rs:53`
- `new` — `src/engine/process.rs:33`
- `with_working_dir` — `src/engine/process.rs:46`

## unit-src-53

- `CommandRecord` — `src/engine/protocol.rs:14`
- `EventRecord` — `src/engine/protocol.rs:60`
- `Inbound` — `src/engine/protocol.rs:91`
- `ResponseRecord` — `src/engine/protocol.rs:48`
- `decode_line` — `src/engine/protocol.rs:97`
- `encode` — `src/engine/protocol.rs:39`
- `encode_line` — `src/engine/protocol.rs:118`
- `get_state` — `src/engine/protocol.rs:30`
- `is_agent_settled` — `src/engine/protocol.rs:68`
- `prompt` — `src/engine/protocol.rs:22`
- `text_delta` — `src/engine/protocol.rs:73`

## unit-src-54

- `Candidate` — `src/integration/mod.rs:16`
- `CandidateStatus` — `src/integration/mod.rs:35`
- `Context` — `src/error.rs:87`
- `DevforgeError` — `src/error.rs:15`
- `IntegrationOutcome` — `src/integration/mod.rs:102`
- `Integrator` — `src/integration/mod.rs:125`
- `MergeQueue` — `src/integration/mod.rs:48`
- `agent` — `src/error.rs:61`
- `enqueue` — `src/integration/mod.rs:80`
- `integrate_next` — `src/integration/mod.rs:145`
- `is_agent_failure` — `src/error.rs:69`
- `is_quality_failure` — `src/error.rs:74`
- `load` — `src/integration/mod.rs:58`
- `queue_path` — `src/integration/mod.rs:72`
- `save` — `src/integration/mod.rs:85`

## unit-src-55

- `change` — `src/lib.rs:10`
- `config` — `src/lib.rs:9`
- `docgen` — `src/lib.rs:11`
- `domain` — `src/lib.rs:12`
- `engine` — `src/lib.rs:13`
- `error` — `src/lib.rs:15`
- `init` — `src/logging.rs:33`
- `integration` — `src/lib.rs:14`
- `log_file_path` — `src/logging.rs:15`
- `logging` — `src/lib.rs:16`
- `observability` — `src/lib.rs:17`
- `open_log_file` — `src/logging.rs:20`
- `runstate` — `src/lib.rs:18`
- `server` — `src/lib.rs:19`
- `sop` — `src/lib.rs:20`
- `workspace` — `src/lib.rs:21`

## unit-src-56

- `EventBus` — `src/observability/events.rs:8`
- `Heartbeat` — `src/observability/heartbeat.rs:16`
- `emit` — `src/observability/events.rs:27`
- `heartbeat_path` — `src/observability/heartbeat.rs:31`
- `last_id` — `src/observability/events.rs:56`
- `new` — `src/observability/events.rs:17`
- `read` — `src/observability/heartbeat.rs:61`
- `replay_after` — `src/observability/events.rs:46`
- `subscribe` — `src/observability/events.rs:41`
- `write` — `src/observability/heartbeat.rs:36`

## unit-src-57

- `HEARTBEAT_REFRESH_MS` — `src/observability/mod.rs:314`
- `PhaseSpan` — `src/observability/mod.rs:136`
- `RunSnapshot` — `src/observability/mod.rs:34`
- `assert_milestone_shape` — `src/observability/mod.rs:478`
- `attempt` — `src/observability/mod.rs:252`
- `begin_run` — `src/observability/mod.rs:84`
- `begin_run_in` — `src/observability/mod.rs:92`
- `begin_run_in_deferred` — `src/observability/mod.rs:106`
- `capture_jsonl` — `src/observability/mod.rs:464`
- `current` — `src/observability/mod.rs:257`
- `end_run` — `src/observability/mod.rs:174`
- `error_chain` — `src/observability/mod.rs:395`
- `events` — `src/observability/mod.rs:408`
- `heartbeat` — `src/observability/mod.rs:406`
- `install` — `src/observability/mod.rs:452`
- `last_agent_event_age_ms` — `src/observability/mod.rs:269`
- `next_review_round` — `src/observability/mod.rs:304`
- `note_agent_event` — `src/observability/mod.rs:264`
- `phase_log` — `src/observability/mod.rs:152`
- `root` — `src/observability/mod.rs:370`
- `run_started_at` — `src/observability/mod.rs:284`
- `set_attempt` — `src/observability/mod.rs:240`
- `set_phase` — `src/observability/mod.rs:192`
- `set_root` — `src/observability/mod.rs:359`
- `set_step` — `src/observability/mod.rs:220`
- `stall_check` — `src/observability/mod.rs:276`
- `stall_warn_ms` — `src/observability/mod.rs:293`
- `test_support` — `src/observability/mod.rs:422`
- `tombstone` — `src/observability/mod.rs:410`
- `touch_run` — `src/observability/mod.rs:345`
- `watchdog` — `src/observability/mod.rs:412`
- `write_heartbeat` — `src/observability/mod.rs:324`

## unit-src-58

- `HookHandle` — `src/observability/tombstone.rs:22`
- `NO_RUN` — `src/observability/tombstone.rs:19`
- `install_hook` — `src/observability/tombstone.rs:44`
- `restore` — `src/observability/tombstone.rs:28`
- `tombstone_path` — `src/observability/tombstone.rs:66`
- `write_tombstone` — `src/observability/tombstone.rs:72`

## unit-src-60

- `Loaded` — `src/runstate/mod.rs:99`
- `RunState` — `src/runstate/mod.rs:59`
- `SCHEMA_VERSION` — `src/runstate/mod.rs:26`
- `STATE_FILE` — `src/runstate/mod.rs:24`
- `StateKey` — `src/runstate/mod.rs:32`
- `StateStore` — `src/runstate/mod.rs:177`
- `clear` — `src/runstate/mod.rs:161`
- `clear` — `src/runstate/mod.rs:209`
- `fresh_state` — `src/runstate/mod.rs:214`
- `key` — `src/runstate/mod.rs:190`
- `load` — `src/runstate/mod.rs:118`
- `load` — `src/runstate/mod.rs:199`
- `matches` — `src/runstate/mod.rs:89`
- `new` — `src/runstate/mod.rs:39`
- `new` — `src/runstate/mod.rs:76`
- `new` — `src/runstate/mod.rs:183`
- `path` — `src/runstate/mod.rs:48`
- `path` — `src/runstate/mod.rs:194`
- `resume` — `src/runstate/mod.rs:17`
- `save` — `src/runstate/mod.rs:136`
- `save` — `src/runstate/mod.rs:204`
- `state` — `src/runstate/mod.rs:109`

## unit-src-61

- `Resumable` — `src/runstate/resume.rs:16`
- `ResumePlan` — `src/runstate/resume.rs:31`
- `StartDecision` — `src/runstate/resume.rs:109`
- `done_units` — `src/runstate/resume.rs:151`
- `plan` — `src/runstate/resume.rs:49`
- `start_gate` — `src/runstate/resume.rs:119`

## unit-src-63

- `CHANGE_PROFILES` — `src/server/change_engine.rs:24`
- `Finalized` — `src/server/change_engine.rs:38`
- `REQUIREMENT_FILE` — `src/server/change_engine.rs:34`
- `build_ctx` — `src/server/change_engine.rs:307`
- `finalize` — `src/server/change_engine.rs:47`
- `run_change` — `src/server/change_engine.rs:66`
- `session_dirs` — `src/server/change_engine.rs:329`

## unit-src-64

- `AnswerReq` — `src/server/handlers.rs:589`
- `ApiError` — `src/server/handlers.rs:22`
- `CreateProjectReq` — `src/server/handlers.rs:92`
- `CreateTaskReq` — `src/server/handlers.rs:162`
- `EnqueueCandidateReq` — `src/server/handlers.rs:812`
- `EventsQuery` — `src/server/handlers.rs:483`
- `RenderGate` — `src/server/gate.rs:14`
- `StartReq` — `src/server/handlers.rs:221`
- `TranscriptQuery` — `src/server/handlers.rs:518`
- `answer_ui` — `src/server/handlers.rs:593`
- `auth_middleware` — `src/server/handlers.rs:74`
- `bad_request` — `src/server/handlers.rs:34`
- `begin_render` — `src/server/gate.rs:31`
- `build_router` — `src/server/http.rs:30`
- `cancel_task` — `src/server/handlers.rs:184`
- `conflict` — `src/server/handlers.rs:31`
- `create_project` — `src/server/handlers.rs:98`
- `create_task` — `src/server/handlers.rs:170`
- `end_render` — `src/server/gate.rs:37`
- `enqueue_candidate` — `src/server/handlers.rs:826`
- `events` — `src/server/handlers.rs:605`
- `get_integration_queue` — `src/server/handlers.rs:854`
- `get_manifest` — `src/server/handlers.rs:640`
- `get_project` — `src/server/handlers.rs:147`
- `get_run_detail` — `src/server/handlers.rs:400`
- `internal` — `src/server/handlers.rs:37`
- `is_in_flight` — `src/server/gate.rs:46`
- `last_agent_event_from_log` — `src/server/handlers.rs:390`
- `list_projects` — `src/server/handlers.rs:142`
- `list_runs` — `src/server/handlers.rs:283`
- `list_tasks` — `src/server/handlers.rs:209`
- `new` — `src/server/gate.rs:22`
- `not_found` — `src/server/handlers.rs:28`
- `parse_token_flag` — `src/server/http.rs:11`
- `put_manifest` — `src/server/handlers.rs:647`
- `read_log_events` — `src/server/handlers.rs:353`
- `read_log_events_at` — `src/server/handlers.rs:363`
- `resolve_startup_token` — `src/server/http.rs:25`
- `run_events` — `src/server/handlers.rs:491`
- `run_transcript` — `src/server/handlers.rs:527`
- `serve` — `src/server/http.rs:82`
- `start_task` — `src/server/handlers.rs:229`
- `task_graph` — `src/server/handlers.rs:314`
- `wait_for_profiles` — `src/server/gate.rs:57`

## unit-src-65

- `RelayUserLink` — `src/server/mod.rs:108`
- `ServerState` — `src/server/mod.rs:122`
- `UiRelay` — `src/server/mod.rs:40`
- `abort_all` — `src/server/mod.rs:99`
- `answer` — `src/server/mod.rs:77`
- `ask_blocking` — `src/server/mod.rs:57`
- `change_engine` — `src/server/mod.rs:8`
- `emit` — `src/server/mod.rs:188`
- `gate` — `src/server/mod.rs:9`
- `handlers` — `src/server/mod.rs:10`
- `http` — `src/server/mod.rs:11`
- `new` — `src/server/mod.rs:48`
- `new` — `src/server/mod.rs:155`
- `pending_count` — `src/server/mod.rs:92`
- `run_in_background` — `src/server/mod.rs:199`

## unit-src-67

- `CODING_REMEDIATION_PREFIX` — `src/sop/coding.rs:62`
- `CONTINUE_NUDGE` — `src/sop/coding.rs:54`
- `CodingReport` — `src/sop/coding.rs:86`
- `CodingSopLoop` — `src/sop/coding.rs:95`
- `MAX_CONTINUE_NUDGES` — `src/sop/coding.rs:59`
- `MAX_PROTOCOL_VIOLATIONS` — `src/sop/coding.rs:82`
- `coding_kickoff` — `src/sop/coding.rs:32`
- `coding_review_instruction` — `src/sop/coding.rs:43`
- `new` — `src/sop/coding.rs:103`
- `run` — `src/sop/coding.rs:114`
- `tail` — `src/sop/coding.rs:24`

## unit-src-68

- `BLOCKERS_DIR` — `src/sop/coding_protocol.rs:25`
- `CODING_DIR` — `src/sop/coding_protocol.rs:15`
- `DOCS_DIR` — `src/sop/coding_protocol.rs:21`
- `NextAction` — `src/sop/coding_protocol.rs:64`
- `REPORTS_DIR` — `src/sop/coding_protocol.rs:23`
- `ReviewReport` — `src/sop/coding_protocol.rs:75`
- `STAGES_FILE` — `src/sop/coding_protocol.rs:17`
- `STAGE_COMPLETE_TAG` — `src/sop/coding_protocol.rs:30`
- `StageVerdict` — `src/sop/coding_protocol.rs:57`
- `TODO_FILE` — `src/sop/coding_protocol.rs:19`
- `parse_review_report` — `src/sop/coding_protocol.rs:96`
- `parse_stage_complete` — `src/sop/coding_protocol.rs:37`

## unit-src-70

- `DOC_FIX_REMEDIATION_PREFIX` — `src/sop/doc/fix.rs:45`
- `DocFixReport` — `src/sop/doc/fix.rs:52`
- `DocFixSopLoop` — `src/sop/doc/fix.rs:59`
- `MAX_DOC_PROTOCOL_VIOLATIONS` — `src/sop/doc/fix.rs:48`
- `doc_review_instruction` — `src/sop/doc/fix.rs:34`
- `docfix_kickoff` — `src/sop/doc/fix.rs:21`
- `new` — `src/sop/doc/fix.rs:67`
- `produced_files` — `src/sop/doc/evidence.rs:40`
- `run` — `src/sop/doc/fix.rs:75`
- `snapshot_files` — `src/sop/doc/evidence.rs:9`

## unit-src-71

- `BASE_SHA` — `src/sop/doc/incremental.rs:17`
- `INCREMENTAL` — `src/sop/doc/incremental.rs:19`
- `Mode` — `src/sop/doc/incremental.rs:23`
- `advance_base` — `src/sop/doc/incremental.rs:94`
- `consistency_notes` — `src/sop/doc/incremental.rs:183`
- `decide_mode` — `src/sop/doc/incremental.rs:46`
- `enabled_from_env` — `src/sop/doc/incremental.rs:31`
- `head_sha` — `src/sop/doc/incremental.rs:167`
- `merge_plan` — `src/sop/doc/incremental.rs:109`
- `set_base` — `src/sop/doc/incremental.rs:85`
- `state_base` — `src/sop/doc/incremental.rs:75`

## unit-src-73

- `DOCS_DIR` — `src/sop/doc/prompts.rs:6`
- `DOC_REMEDIATION_PREFIX` — `src/sop/doc/prompts.rs:11`
- `DocReviewReport` — `src/sop/doc/protocol.rs:65`
- `DocRoute` — `src/sop/doc/protocol.rs:33`
- `DocVerdict` — `src/sop/doc/protocol.rs:27`
- `MAX_DOC_CONTINUE_NUDGES` — `src/sop/doc/protocol.rs:17`
- `MAX_DOC_EMPTY_REPLIES` — `src/sop/doc/protocol.rs:21`
- `MAX_DOC_PROTOCOL_VIOLATIONS` — `src/sop/doc/protocol.rs:13`
- `MAX_DOC_REVIEW_RETRIES` — `src/sop/doc/protocol.rs:24`
- `META_DIR` — `src/sop/doc/prompts.rs:8`
- `MockAgent` — `src/sop/doc/mocks.rs:23`
- `NoUser` — `src/sop/doc/mocks.rs:16`
- `PipeAgent` — `src/sop/doc/mocks.rs:75`
- `PipeReviewFactory` — `src/sop/doc/mocks.rs:107`
- `as_str` — `src/sop/doc/protocol.rs:42`
- `doc_final_instruction` — `src/sop/doc/prompts.rs:42`
- `doc_kickoff` — `src/sop/doc/prompts.rs:14`
- `doc_review_instruction` — `src/sop/doc/prompts.rs:27`
- `drive_gates` — `src/sop/doc/mocks.rs:143`
- `evidence` — `src/sop/doc/mod.rs:12`
- `fix` — `src/sop/doc/mod.rs:13`
- `gate_pair` — `src/sop/doc/mocks.rs:158`
- `incremental` — `src/sop/doc/mod.rs:14`
- `new` — `src/sop/doc/mocks.rs:28`
- `new` — `src/sop/doc/mocks.rs:80`
- `no_gates` — `src/sop/doc/mocks.rs:165`
- `parse` — `src/sop/doc/protocol.rs:52`
- `parse_doc_complete` — `src/sop/doc/protocol.rs:75`
- `parse_doc_review` — `src/sop/doc/protocol.rs:86`
- `pipe_factory` — `src/sop/doc/mocks.rs:43`
- `prompts` — `src/sop/doc/mod.rs:15`
- `protocol` — `src/sop/doc/mod.rs:16`
- `resume` — `src/sop/doc/mod.rs:17`
- `runner` — `src/sop/doc/mod.rs:18`
- `seed_done` — `src/sop/doc/mocks.rs:61`
- `state` — `src/sop/doc/mod.rs:19`
- `state_path` — `src/sop/doc/mocks.rs:53`
- `wiring` — `src/sop/doc/mod.rs:20`

## unit-src-74

- `DOC_SOP` — `src/sop/doc/resume.rs:14`
- `DocResume` — `src/sop/doc/resume.rs:20`
- `files_of` — `src/sop/doc/resume.rs:41`
- `new` — `src/sop/doc/resume.rs:28`
- `with_lint` — `src/sop/doc/resume.rs:35`

## unit-src-76

- `DocReport` — `src/sop/doc/runner.rs:32`
- `run_pipeline` — `src/sop/doc/runner.rs:106`

## unit-src-79

- `COMPLETION_TAG` — `src/sop/output.rs:10`
- `Verdict` — `src/sop/output.rs:19`
- `add_issues` — `src/sop/doc/state.rs:78`
- `bump_review` — `src/sop/doc/state.rs:63`
- `coding` — `src/sop/mod.rs:12`
- `coding_protocol` — `src/sop/mod.rs:13`
- `doc` — `src/sop/mod.rs:14`
- `done_units` — `src/sop/doc/state.rs:18`
- `is_complete` — `src/sop/output.rs:13`
- `is_done` — `src/sop/doc/state.rs:32`
- `issues_total` — `src/sop/doc/state.rs:83`
- `mark_done` — `src/sop/doc/state.rs:43`
- `output` — `src/sop/mod.rs:15`
- `parse_verdict` — `src/sop/output.rs:57`
- `planning` — `src/sop/mod.rs:16`
- `review_gate` — `src/sop/mod.rs:17`
- `review_rounds` — `src/sop/doc/state.rs:69`
- `rpc_bridge` — `src/sop/mod.rs:18`
- `testing` — `src/sop/mod.rs:19`
- `testing_protocol` — `src/sop/mod.rs:20`
- `units_total` — `src/sop/doc/state.rs:23`
- `unmark_done` — `src/sop/doc/state.rs:37`
- `violation_reason` — `src/sop/output.rs:30`

## unit-src-80

- `PLAN_KICKOFF` — `src/sop/planning.rs:19`
- `PlanAgentSession` — `src/sop/planning.rs:44`
- `PlanningLoop` — `src/sop/planning.rs:81`
- `REMEDIATION_PREFIX` — `src/sop/planning.rs:35`
- `REVIEW_INSTRUCTION` — `src/sop/planning.rs:27`
- `ReviewSession` — `src/sop/planning.rs:56`
- `ReviewSessionFactory` — `src/sop/planning.rs:62`
- `StageReport` — `src/sop/planning.rs:74`
- `UserLink` — `src/sop/planning.rs:39`
- `new` — `src/sop/planning.rs:87`
- `run` — `src/sop/planning.rs:95`

## unit-src-81

- `MAX_PROTOCOL_VIOLATIONS` — `src/sop/review_gate.rs:16`
- `MAX_REVIEW_INSTANCE_FAILURES` — `src/sop/review_gate.rs:19`
- `ReviewBudget` — `src/sop/review_gate.rs:26`
- `ReviewOutcome` — `src/sop/review_gate.rs:34`
- `outcome_of_review_report` — `src/sop/review_gate.rs:60`
- `review_with_gate` — `src/sop/review_gate.rs:78`
- `verdict` — `src/sop/review_gate.rs:43`
- `violation` — `src/sop/review_gate.rs:51`

## unit-src-82

- `MAX_TEST_CONTINUE_NUDGES` — `src/sop/testing.rs:55`
- `MAX_TEST_PROTOCOL_VIOLATIONS` — `src/sop/testing.rs:77`
- `RpcPlanSession` — `src/sop/rpc_bridge.rs:13`
- `RpcReviewFactory` — `src/sop/rpc_bridge.rs:94`
- `RpcReviewSession` — `src/sop/rpc_bridge.rs:69`
- `TESTING_CONTINUE_NUDGE` — `src/sop/testing.rs:50`
- `TESTING_REMEDIATION_PREFIX` — `src/sop/testing.rs:58`
- `TestingOutcome` — `src/sop/testing.rs:94`
- `TestingReport` — `src/sop/testing.rs:81`
- `TestingSopLoop` — `src/sop/testing.rs:117`
- `new` — `src/sop/rpc_bridge.rs:18`
- `new` — `src/sop/rpc_bridge.rs:100`
- `new` — `src/sop/testing.rs:125`
- `per_round` — `src/sop/rpc_bridge.rs:107`
- `per_round_in` — `src/sop/rpc_bridge.rs:121`
- `resume_after_docfix` — `src/sop/testing.rs:147`
- `run` — `src/sop/testing.rs:133`
- `testing_kickoff` — `src/sop/testing.rs:27`
- `testing_review_instruction` — `src/sop/testing.rs:39`

## unit-src-84

- `COVERAGE_MATRIX_FILE` — `src/sop/testing_protocol.rs:22`
- `DocReviewReport` — `src/sop/testing_protocol.rs:263`
- `FINDINGS_DIR` — `src/sop/testing_protocol.rs:20`
- `FindingRecord` — `src/sop/testing_protocol.rs:78`
- `FindingStatus` — `src/sop/testing_protocol.rs:43`
- `Reroute` — `src/sop/testing_protocol.rs:134`
- `TESTING_BLOCKERS_DIR` — `src/sop/testing_protocol.rs:26`
- `TESTING_DIR` — `src/sop/testing_protocol.rs:14`
- `TESTING_REPORTS_DIR` — `src/sop/testing_protocol.rs:24`
- `TESTING_SKELETON_DIRS` — `src/sop/testing_protocol.rs:29`
- `TESTING_STAGES_FILE` — `src/sop/testing_protocol.rs:16`
- `TESTING_TODO_FILE` — `src/sop/testing_protocol.rs:18`
- `TestReviewReport` — `src/sop/testing_protocol.rs:159`
- `as_str` — `src/sop/testing_protocol.rs:66`
- `as_str` — `src/sop/testing_protocol.rs:149`
- `create_testing_skeleton` — `src/sop/testing_protocol.rs:32`
- `parse` — `src/sop/testing_protocol.rs:57`
- `parse` — `src/sop/testing_protocol.rs:142`
- `parse_doc_review_report` — `src/sop/testing_protocol.rs:272`
- `parse_finding_record` — `src/sop/testing_protocol.rs:97`
- `parse_test_review_report` — `src/sop/testing_protocol.rs:174`

## unit-src-86

- `OrphanWorkspace` — `src/workspace/mod.rs:149`
- `Workspace` — `src/workspace/mod.rs:31`
- `WorkspaceSpec` — `src/workspace/mod.rs:20`
- `create` — `src/workspace/mod.rs:95`
- `ensure` — `src/workspace/mod.rs:53`
- `freeze` — `src/workspace/mod.rs:128`
- `remove` — `src/workspace/mod.rs:135`
- `remove_orphan` — `src/workspace/mod.rs:180`
- `scan_orphans` — `src/workspace/mod.rs:158`
- `target_path` — `src/workspace/mod.rs:81`

## unit-tests-91

- `Reporter` — `tests/e2e/common.py:434`
- `Server` — `tests/e2e/common.py:52`
- `SseStream` — `tests/e2e/common.py:152`
- `UiAutoPilot` — `tests/e2e/common.py:338`
- `base` — `tests/e2e/common.py:71`
- `check` — `tests/e2e/common.py:443`
- `close` — `tests/e2e/common.py:214`
- `compensate_profile` — `tests/e2e/common.py:238`
- `finish` — `tests/e2e/common.py:459`
- `free_port` — `tests/e2e/common.py:44`
- `issue` — `tests/e2e/common.py:452`
- `mkdtemp_home` — `tests/e2e/common.py:36`
- `real_api_key` — `tests/e2e/common.py:228`
- `req` — `tests/e2e/common.py:132`
- `start` — `tests/e2e/common.py:74`
- `start` — `tests/e2e/common.py:350`
- `start_tolerant` — `tests/e2e/common.py:418`
- `stop` — `tests/e2e/common.py:120`
- `stop` — `tests/e2e/common.py:353`
- `wait_for` — `tests/e2e/common.py:201`
- `wait_run_status` — `tests/e2e/common.py:403`

## unit-tests-92

- `discover_port` — `tests/e2e/dialog_responder.py:15`
- `generate_report` — `tests/e2e/run_all.py:83`
- `git_rev` — `tests/e2e/run_all.py:140`
- `main` — `tests/e2e/run_all.py:46`
- `main` — `tests/e2e/test_00_server.py:17`
- `main` — `tests/e2e/test_01_projects.py:12`
- `main` — `tests/e2e/test_02_tasks.py:11`
- `main` — `tests/e2e/test_03_config.py:13`
- `pick_value` — `tests/e2e/dialog_responder.py:59`
- `post_answer` — `tests/e2e/dialog_responder.py:43`
- `watch` — `tests/e2e/dialog_responder.py:73`

## unit-tests-93

- `dump_session_tail` — `tests/e2e/test_08_e2e_planning.py:40`
- `main` — `tests/e2e/test_04_auth.py:15`
- `main` — `tests/e2e/test_05_events_sse.py:11`
- `main` — `tests/e2e/test_06_artifacts_graph.py:11`
- `main` — `tests/e2e/test_07_rerun_serial.py:26`
- `main` — `tests/e2e/test_08_e2e_planning.py:69`
- `main` — `tests/e2e/test_09_run_control.py:20`
- `main` — `tests/e2e/test_10_timeout_async_render.py:21`
- `main` — `tests/e2e/test_11_coding_config.py:24`
- `session_bytes` — `tests/e2e/test_08_e2e_planning.py:26`
- `session_bytes` — `tests/e2e/test_08_e2e_planning.py:33`

## unit-tests-94

- `find_run_pi` — `tests/e2e/test_16_run_liveness.py:22`
- `latest_run` — `tests/e2e/test_16_run_liveness.py:49`
- `main` — `tests/e2e/test_12_sop_override.py:21`
- `main` — `tests/e2e/test_13_coding_preflight.py:21`
- `main` — `tests/e2e/test_14_coding_full_chain.py:47`
- `main` — `tests/e2e/test_15_coding_run_control.py:19`
- `main` — `tests/e2e/test_16_run_liveness.py:55`
- `main` — `tests/e2e/test_17_testing_config.py:37`
- `main` — `tests/e2e/test_18_sop_registry.py:21`
- `main` — `tests/e2e/test_19_testing_full_chain.py:55`
- `newest_session_bytes` — `tests/e2e/test_14_coding_full_chain.py:32`
- `newest_session_bytes` — `tests/e2e/test_19_testing_full_chain.py:49`
- `stop_pid` — `tests/e2e/test_16_run_liveness.py:42`
- `wait_run_finished` — `tests/e2e/test_14_coding_full_chain.py:38`
- `wait_runs_terminal` — `tests/e2e/test_18_sop_registry.py:66`
- `wait_task_sop` — `tests/e2e/test_18_sop_registry.py:53`

## unit-tests-95

- `add_book` — `tests/e2e/test_22_doc_from_zero.py:60`
- `alpha_extra` — `tests/e2e/test_23_doc_incremental.py:45`
- `alpha_sum` — `tests/e2e/test_23_doc_incremental.py:30`
- `alpha_sum` — `tests/e2e/test_24_doc_resume.py:28`
- `beta_count` — `tests/e2e/test_23_doc_incremental.py:38`
- `beta_count` — `tests/e2e/test_24_doc_resume.py:36`
- `compute` — `tests/e2e/test_20_testing_fixture.py:54`
- `git` — `tests/e2e/test_23_doc_incremental.py:51`
- `list_books` — `tests/e2e/test_22_doc_from_zero.py:69`
- `load_state` — `tests/e2e/test_23_doc_incremental.py:76`
- `load_state` — `tests/e2e/test_24_doc_resume.py:54`
- `main` — `tests/e2e/test_20_testing_fixture.py:68`
- `main` — `tests/e2e/test_20_testing_fixture.py:147`
- `main` — `tests/e2e/test_21_coding_to_testing.py:78`
- `main` — `tests/e2e/test_22_doc_from_zero.py:43`
- `main` — `tests/e2e/test_22_doc_from_zero.py:88`
- `main` — `tests/e2e/test_23_doc_incremental.py:137`
- `main` — `tests/e2e/test_24_doc_resume.py:91`
- `newest_report` — `tests/e2e/test_23_doc_incremental.py:105`
- `newest_session_bytes` — `tests/e2e/test_20_testing_fixture.py:141`
- `newest_session_bytes` — `tests/e2e/test_21_coding_to_testing.py:57`
- `newest_session_bytes` — `tests/e2e/test_22_doc_from_zero.py:82`
- `parse_tokens` — `tests/e2e/test_23_doc_incremental.py:115`
- `run_once` — `tests/e2e/test_23_doc_incremental.py:83`
- `session_inventory` — `tests/e2e/test_24_doc_resume.py:72`
- `snapshot_files` — `tests/e2e/test_23_doc_incremental.py:124`
- `state_path` — `tests/e2e/test_23_doc_incremental.py:64`
- `state_path` — `tests/e2e/test_24_doc_resume.py:42`
- `unit_files` — `tests/e2e/test_24_doc_resume.py:61`
- `wait_run` — `tests/e2e/test_21_coding_to_testing.py:63`
- `wait_run_final` — `tests/e2e/test_24_doc_resume.py:80`

## unit-tests-96

- `FaultProxy` — `tests/e2e/test_25_fault_retry.py:58`
- `Handler` — `tests/e2e/test_25_fault_retry.py:72`
- `NormalizeWhitespaceTests` — `tests/e2e/test_26_change_e2e.py:58`
- `SqueezeSpacesTests` — `tests/e2e/test_26_change_e2e.py:66`
- `alpha_sum` — `tests/e2e/test_25_fault_retry.py:44`
- `assemble_change_manifest` — `tests/e2e/test_26_change_e2e.py:119`
- `base` — `tests/e2e/test_25_fault_retry.py:158`
- `beta_count` — `tests/e2e/test_25_fault_retry.py:52`
- `do_GET` — `tests/e2e/test_25_fault_retry.py:143`
- `do_POST` — `tests/e2e/test_25_fault_retry.py:140`
- `fails` — `tests/e2e/test_25_fault_retry.py:161`
- `forwards` — `tests/e2e/test_25_fault_retry.py:164`
- `git` — `tests/e2e/test_26_change_e2e.py:106`
- `load_state` — `tests/e2e/test_25_fault_retry.py:189`
- `log_message` — `tests/e2e/test_25_fault_retry.py:73`
- `main` — `tests/e2e/test_25_fault_retry.py:204`
- `main` — `tests/e2e/test_26_change_e2e.py:136`
- `normalize_whitespace` — `tests/e2e/test_26_change_e2e.py:39`
- `patch_base_url` — `tests/e2e/test_25_fault_retry.py:168`
- `squeeze_spaces` — `tests/e2e/test_26_change_e2e.py:44`
- `start` — `tests/e2e/test_25_fault_retry.py:151`
- `stop` — `tests/e2e/test_25_fault_retry.py:154`
- `test_basic` — `tests/e2e/test_26_change_e2e.py:59`
- `test_empty` — `tests/e2e/test_26_change_e2e.py:62`
- `test_keeps_newline` — `tests/e2e/test_26_change_e2e.py:70`
- `test_only_spaces` — `tests/e2e/test_26_change_e2e.py:67`
- `walk` — `tests/e2e/test_25_fault_retry.py:174`

## unit-tests-97

- `AssembleChangeManifestTests` — `tests/e2e/test_27_change_ask_ring.py:41`
- `FakeServer` — `tests/e2e/test_27_change_ask_ring.py:122`
- `FakeSse` — `tests/e2e/test_27_change_ask_ring.py:133`
- `UiAutoPilotAnswerTests` — `tests/e2e/test_27_change_ask_ring.py:152`
- `load_fixture` — `tests/e2e/test_27_change_ask_ring.py:37`
- `req` — `tests/e2e/test_27_change_ask_ring.py:128`
- `select_event` — `tests/e2e/test_27_change_ask_ring.py:138`
- `test_answered_records_the_posted_value` — `tests/e2e/test_27_change_ask_ring.py:165`
- `test_change_agents_are_appended_when_host_has_none` — `tests/e2e/test_27_change_ask_ring.py:92`
- `test_host_manifest_already_containing_change_agents_is_overridden` — `tests/e2e/test_27_change_ask_ring.py:44`
- `test_other_host_agents_are_untouched` — `tests/e2e/test_27_change_ask_ring.py:105`
- `test_smart_answer_picks_the_option_label_not_the_default_answer` — `tests/e2e/test_27_change_ask_ring.py:155`

## unit-tests-98

- `support` — `tests/f6_observability/main.rs:43`

