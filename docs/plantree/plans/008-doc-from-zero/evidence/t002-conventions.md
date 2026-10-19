# T002 · 约定面抽取（配置键 / 数据层 / 构建启动命令）

**结论**：清单从「模块 + 符号 + 入口」扩成「+ 配置项 + 数据对象 + 行为命令」，全部引擎侧、零依赖、可单测。
在 devforge 自身上实测：`config 12 / data 0 / behavior 4`（behavior = Makefile 目标 + package.json scripts；data 0 因为本仓没有 SQL DDL）。

## 抽什么、为什么这么抽

| 面 | 抽取源 | 为什么不是别的 |
| --- | --- | --- |
| 配置项 Config | 源码里的 env 读取（`os.environ.get` / `os.getenv` / `process.env` / `os.Getenv` / `env::var`）+ `.env.example` / `.env.sample` | 「代码真读了什么」比「配置文件里写了什么」更接近真相——写了没读的键是噪音，读了没写进样板文件的键才是事故点 |
| 数据对象 Data | `*.sql` 里的 `CREATE TABLE` / `CREATE VIEW` | DDL 是唯一不带框架味的真相源；ORM 模型名会骗人（模型名 ≠ 表名） |
| 行为命令 Behavior | `Makefile` 目标 + `package.json` scripts | 这两处是「人类告诉你怎么跑」的地方，正好对上 Runbook 七件套的「启停与健康检查」 |

## 单元归属（关键一致性）

新增条目也必须挂 `unit_id`，否则单元审永远覆盖不到它们（空 unit 的条目在单元级对账里是隐形人）。
规则：文件本身有模块条目 → 同单元；无模块条目（`.env.example` / 根级 `Makefile`）→ 按顶层目录分组分配新单元。
新增单测 `convention_entries_land_in_inventory_with_units` 就是钉这条的：`all(|e| !e.unit_id.is_empty())`。

## 档位联动（D007）

`DocLevel` 的机械覆盖口径同步调整——否则抽了不算账：

- Essential：仅 Entry
- Complete（默认）：Entry + Module + Symbol + **Config** + **Data**
- Detailed：以上 + **Behavior**

## 已知边界（v1 不装懂）

- 不解析 `pyproject.toml` / `docker-compose.yml`（TOML 与 YAML 都要额外依赖，收益不抵复杂度）；
- DDL 只认 `CREATE TABLE/VIEW`，不认 `ALTER`（变更历史归 gaps 或人工）；
- JS 的 `process.env.X` 若动态拼写（`process.env[key]`）抽不到——属于启发式固有盲区，登记为精度缺口而非分母。

## 门禁

`cargo test --lib` 262 通过（新增 5 条）；`cargo clippy --all-targets` 0 警告。
