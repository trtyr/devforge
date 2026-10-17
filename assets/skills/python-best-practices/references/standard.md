# Python AI 编码规范

> **目标**:用机器可读的类型契约 + 测试,给 AI agent 一个能自主收敛的反馈闭环,最大化自动化编码的效率与可靠性。
>
> 适用对象:人类开发者 **和** AI 编码 agent(可直接作为 `CLAUDE.md` / `AGENTS.md` 的一部分)。
>
> **技术基线(只用当前主流大版本,不背历史包袱):** Python **3.13** · mypy `--strict` · ruff · beartype · pydantic v2 + pydantic-settings · Jinja2 · Hypothesis。
> 一律现代写法:**不写** `from __future__ import annotations`(理由见 §6,已由 ruff `TID251` 门禁强制——前向/自引用类型按需加引号即可);**不用** `typing.List/Dict/Optional`;优先 PEP 695 的 `type` 别名与 `class Box[T]` 泛型语法。

---

## 0. 核心原则

AI agent 自主编码的瓶颈是**反馈信号的质量**。动态 Python 信号太弱:代码能跑通却悄悄做错事,agent 无法判断对错、无法自我纠正。本规范的全部目的就是补强这个信号:

- **mypy(静态)** —— 不运行就覆盖所有分支,给出编译期错误。
- **beartype(运行时)** —— 在实际执行路径上验证连静态层面都看不见的真实值(LLM 输出、反序列化数据)。
- **测试** —— 验证类型管不了的**行为**。

三者合一,把"可能不对"变成**精确到参数、可被 agent 直接解析并修复**的错误。类型越完整,agent 的错误假设越少、修复循环收敛越快。

> ⚠️ 类型约束的是**形状(shape)**,不是**行为(behavior)**。类型全对、逻辑写错的函数依然存在。类型是底座,测试管行为,缺一不可。

---

## 1. 反馈闭环:一条命令的质量门

整个自动化的核心原语是**一条命令**,把所有检查串起来、输出机器可读结果。agent 每改一次代码就跑一次,解析错误,修复,重复,直到全绿——这一步不需要人介入。

按"快→慢"排,让 agent 尽早拿到反馈:

1. `ruff format --check` —— 格式
2. `ruff check` —— lint。选进来的规则集是把本规范的散文条款逐条兑成门禁:`ANN`(强制类型注解)、`TID251`(禁 `from __future__ import annotations`)、`TID252`(包内禁相对导入,§7.1)、`BLE`(禁 `except Exception` 兜底,§9)、`T20`(禁 `print`,§7.4 的"库代码只 getLogger")、`PGH`(禁裸 `type: ignore` / `noqa`)。**唯一豁免是 `scripts/*.py` 的 `T20`**——门检查器是命令行工具层,它的输出契约就是 stdout;那是按目录划定的配置,不是散落的 `noqa`
3. `mypy` —— 静态类型(全分支)
4. `scripts/check_conformance.py` —— 结构不变量(beartype hook、零 SDK 外泄、绝对导入、`core/` 叶子约束)
5. `scripts/check_drift.py` —— 文档漂移
6. `pytest`(beartype 激活)—— 运行时类型 + 行为

具体命令见[附录](#附录配置与样板)。结构检查器随项目出厂(scaffold 会把它复制进项目 `scripts/`)并挂在门里——**不挂进门的检查器等于不存在**:实测把 beartype hook 整段删掉,只要检查器不在门里,`./ci.sh` 照样全绿。**禁止**让 agent 用零散的、需人工判读的检查方式;必须收敛到这一条命令。它是 agent 唯一的正确性判据,不允许"看起来没问题就提交"。

---

## 2. 静态类型 — mypy `--strict`

- 每个函数的**参数和返回值**都必须有类型注解,不留空(`--strict` 强制)。
- **禁止裸 `Any`**。需要动态时用 `object`、`Protocol`、`TypeVar`、`Union`、`Literal` 收窄。
- 泛型必须参数化:`list[str]` 而非裸 `list`,`dict[str, int]` 而非裸 `dict`。
- 类型别名用 PEP 695 的 `type` 语句:`type UserId = int`(不用 `TypeAlias`)。
- `# type: ignore` 必须带**错误码和理由**(`# type: ignore[arg-type]  # 原因…`),视为待还的技术债;`--strict` 下失效的 ignore 会直接报错,升级依赖时记得清理。**带错误码这条已有门禁**:`[tool.mypy] enable_error_code = ["ignore-without-code"]`,裸 `# type: ignore` 直接红。裸写法的真实危害是它顺手关掉**同一行上所有**别的错误,而且没人知道它当初在压什么。
- `__init__.py` 的转发要显式(`from myproj.retrieval.reranking import Reranker as Reranker` 或 `__all__`),否则 `no-implicit-reexport` 下别人 import 不到。注意仍是绝对导入(§7.1)。

新代码从严无例外。若并入存量代码,用 `[[tool.mypy.overrides]]` 按模块临时降级。

---

## 3. 运行时类型 — beartype

**机制:中心化布尔开关(pydantic-settings)+ `beartype.claw` import hook。** 不手动写 `@beartype` 装饰器,**绝不靠注释/反注释源码**来开关——那是不可自动化的体力活,污染 git diff,回归调试还得逐个改回。

开关由 `core/settings.py` 里的 `Settings` 暴露(`beartype_on`,**默认 `True`**;仅生产环境显式设 `APP_BEARTYPE_ON=false` 关闭),由包的 `__init__.py` 读取并条件安装 claw hook。代码见[附录](#附录配置与样板)。

### ❗ 关键约束:import 顺序(最容易踩的坑)

claw hook 只 instrument 它**安装之后**才被 import 的模块,已 import 的不会被追溯处理。由此三条铁律:

1. **hook 必须在 `__init__.py` 最顶部安装**,后续导入的子模块才会被检查。
2. **hook 之前的 import 面要最小**:在它之前只能导入 `settings`。因此 `core/settings.py` 必须是**叶子模块**——只依赖标准库 + 第三方配置库(pydantic-settings、PyYAML),**不 import 任何想被检查的一方代码**。第三方依赖不受此约束。
3. **`core/__init__.py` 必须保持空**:导入 settings 会先经过它,若在此 re-export `logging`/`prompts`,这些会在 hook 之前被导入、从而漏检。

> **为什么放 `__init__.py` 就够,不用在每个入口重复装 hook:** 任何对 `myproj` 的导入都会**先**执行 `myproj/__init__.py`,所以它是所有调用路径上唯一、保证最先跑的钩子点——cli、scripts、tests、别的包导入它,都自动覆盖。**前提是"从包进入"**:用 `import myproj.X` / `python -m myproj.cli` / console_script / `uv run`。**别直接跑散文件**(`python src/myproj/cli.py`)——包内绝对导入(§7.1)让散文件跑法不再崩,`__init__.py` 也照样被触发、hook 照样装上,但**那个入口文件自身是以 `__main__` 身份在 hook 之前加载的,它自己定义的函数不被 instrument**,而且同一份代码会以 `__main__` 和 `myproj.cli` 两个身份各存在一份。所以散文件跑法只是"能跑",不是"该跑"。另外 hook 进程内只装一次:pre-hook 加载过的模块会被 `sys.modules` 缓存、整个进程不再被检查,所以 pre-hook 面要严格只有 settings。

### ❗ 检查强度只有一档:O(1) 抽样。不要相信"CI 全量"

**只有一个总开关(on/off),没有强度分档。** beartype 的 `BeartypeStrategy.On`(线性、逐元素全量)**至今未实现**——官方源码里就标着 "currently unimplemented",`On` 与 `O1` 生成的 wrapper 逐字节相同。任何"本地 `O1` 抽样、CI 用 `On` 全量抓干净"的说法(包括本规范早期版本、以及网上大量教程)都是 **no-op**:`CI=1` 什么也没改变。

所以实际语义只有两种状态:

| 环境 | beartype | 由谁控制 | 实际语义 |
|---|---|---|---|
| 开发 / 测试 / CI | 开启 | 默认(`beartype_on=True`) | **O(1) 抽样**:容器只随机抽**一个**元素做检查 |
| 生产 | 关闭 | `APP_BEARTYPE_ON=false` | 零开销;边界安全交给 pydantic(§4) |

诚实的心智模型:**beartype 抓的是"形状跑偏"(类型完全错、层级错),不是"容器里第 7 个元素是脏的"。** `list[str]` 里混进一个 `int`,单次调用有很大概率抽不到它,重跑一次结论还可能不同。

**别指望调高强度来兜住这件事——那条路根本不存在。** 容器深处的坏数据由**边界处的 pydantic 校验**兜底(§4):所有穿过进程边界的数据(LLM 输出、工具结果、HTTP、反序列化)必须在入口 `model_validate`,pydantic 是**逐元素**校验的。这也正是"边界 pydantic / 内部 beartype"这条分工的真正原因,而不只是风格偏好。

---

## 4. 边界 — pydantic

**所有穿过进程边界的数据,都必须在入口处建模并校验**,不要信任它符合声明的类型:LLM 结构化输出、工具调用返回、HTTP 载荷、配置文件、用户输入、任何反序列化结果。

```python
from pydantic import BaseModel

class LLMToolCall(BaseModel):
    name: str
    arguments: dict[str, object]

call = LLMToolCall.model_validate(raw_json_from_llm)  # 在边界 parse,而非信任
```

**beartype vs pydantic 决策规则:**

- **穿过进程边界的数据 → pydantic**:需要校验合法性,常常还要转换/收窄(字符串转 datetime、约束值范围)。
- **进程内部的契约 → beartype**:数据已在掌控内,只想运行时断言"形状没跑偏"作为保险,不需转换。

RAG/agent 流水线里,LLM 输出和工具结果在 pydantic 边界一卡,正好抓住 mypy 看不见、最容易出 bug 的那一类(模型返回结构跑歪)。

---

## 5. 测试 — 让运行时检查真正生效

beartype **只能检查实际执行到的路径**,所以它的有效性 = 测试覆盖。

- **边界优先**:对所有 pydantic 边界,用真实样本 + **对抗样本**(缺字段、类型错、空值、超长)测试。
- **Hypothesis(property-based testing)是放大器**:自动生成大量输入 → 把各种路径都跑一遍 → beartype 在每次调用上验证类型 → 自动发现人工想不到的违例。这套组合直接实现 agent "自己发现 bug"。
- 覆盖率不是目标本身,但**没跑到的分支 beartype 管不着**,核心路径的覆盖要保证。
- **配置分支要能测**:用 `tests/conftest.py` 的 `override_settings` fixture 在单个测试内换配置。

  ```python
  def test_provider_switches(override_settings: Callable[..., Settings]) -> None:
      override_settings(APP_LLM_PROVIDER="openai")   # 变量名就是 APP_ 前缀那套
      assert isinstance(make_llm(), OpenAILLM)       # 嵌套字段用 APP_RETRIEVER__TOP_K
  ```

  它设完环境变量再 `get_settings.cache_clear()`,teardown 时清缓存并由 monkeypatch 还原环境。
  刻意走真实的「env → `Settings`」链路而不是往生产代码里塞注入点:测的就是部署时实际生效的
  那条路径,顺带证明 §7.5 的优先级链没写错。**没有这条缝,provider 分支只有默认那条跑得到**
  (成因见 §7.4)。

---

## 6. 代码形态 — 为 AI 可读性服务

类型是 agent 读代码时的接口契约;形态越清晰,agent 假设越少、改得越准。

**现代写法(3.13)**

- 泛型用 PEP 695:`def first[T](xs: list[T]) -> T:`、`class Box[T]: ...`,不再手写 `TypeVar`。
- 类型别名:`type Json = dict[str, "Json"] | list["Json"] | str | int | float | bool | None`。
- 枚举用 `StrEnum`(`from enum import StrEnum`)。
- 覆写方法标 `@override`(`from typing import override`),让签名漂移在静态层暴露。

**禁 `from __future__ import annotations`(PEP 563)**——已由 ruff `TID251` banned-api 门禁强制,写了直接红。

理由不是"它和 beartype 冲突"(那个说法经得起实验证伪:beartype 能处理字符串化注解)。真实理由是三条:

1. **它是被官方放弃的过渡路径。** PEP 563 从未成为默认,最终落地的是 3.14 的 **PEP 649 惰性求值**——注解按需求值成**真对象**,不做字符串化。也就是说 future import 当年要解决的问题,在语言层已经用**另一条路**解决了,而它自己永远不会转正。在 3.13 上,我们的基线里它本就不解决任何问题。
2. **活注解对象是零机关路径。** beartype / pydantic 处理字符串化注解,靠的是各自的求值子系统(拿 `__globals__`、`__module__` 去 `eval`)。多一层机关就多一批边界情况:实测已抓到"习惯性给类型加引号 + future import"造成**双重字符串化**,让 beartype 直接炸。注解保持活对象,这类问题整类消失。
3. **规则统一的代价极小。** 禁掉它的唯一代价是前向引用 / 自引用类型要加引号(`def f(self) -> "Node":`)——本规范的样板本来就这么写。用一条"永远不写"换掉"什么时候写、写了哪些库会不会出事"这一整片判断空间,对 AI agent 尤其划算。

**通用约束**

- **小而专的函数**:既是 beartype/pydantic 的天然边界,也让 agent 容易推理。
- **类型即契约**:签名要能让人/agent 不读函数体就理解接口;配简短 docstring 说明契约。
- **避免 `Any` 和 `# type: ignore`**:每一个都是 mypy、beartype、agent 三方共同的盲区。
- **不要静默失败**:让错误响亮且定位明确。beartype 的违例信息、Jinja2 的 `StrictUndefined` 都是这种"宁可报错"的取向。不要用裸 `except` / `except Exception` 吞掉(ruff `E722` + `BLE` 会红)。

---

## 7. 项目结构

布局参考 lightning-hydra-template,但**去掉 Hydra**(理由见 §7.4),配置走 pydantic-settings。

### 7.1 src 布局 + 真实包名;不用 rootutils,但用 `.project-root` 锚仓库根

- **永远用 src 布局**(`src/<name>/`):PyPA 推荐,强制"装了才能测",能抓打包 bug。
- **一开始就给包起真名,自用也别叫 `import src`。** 自用 vs 上 PyPI 的唯一区别只是这个名字取项目名还是发行名;布局和导入路径都不变,以后抽库/上架零改动。
- **不需要 rootutils,也不碰 `sys.path`。** rootutils 那套是 lightning-hydra-template 把 `src` 当包、且不安装就 `python src/train.py` 的产物,所以非改 `sys.path` 不可。我们**把包装上**(editable),`uv sync` 写下的 `.pth` 已经把 `ROOT/src` 加进解释器路径,`pytest`、`scripts/` 里的脚本同理。给可安装的包用 rootutils 改路径是反模式。
  - 说准确点:`.pth` 让 **import 解析**在任何 CWD 都成立;但 `import myproj` 会执行 `__init__.py`,它一路走到 `_find_project_root()`,**在项目树外(且没设 `APP_ROOT_DIR`)会抛 `RuntimeError`**。所以"任何 CWD 都能 import"只对**树内**成立;**运行**仍需在项目树内,或用 `APP_ROOT_DIR` 显式指定根。这不是缺陷,正是 `.project-root` 锚的设计行为(§7.1 下一条):宁可启动即炸,也不要 `ROOT_DIR` 悄悄变成"你碰巧在哪"。
- **包内一律绝对导入 `from myproj.X import Y`,禁止相对导入(`from .x` / `from ..x`)。** 相对导入要求模块必须"从包进入"才能解析;一旦有人把包内文件当散文件直接跑(`python src/myproj/cli.py`——**PyCharm 里对包内文件右键 Run 就是这个模式**),Python 不知道它的父包,立刻 `ImportError: attempted relative import with no known parent package`。绝对导入在包已 editable 安装的前提下,`python -m myproj.cli` 与散文件跑法**都能工作**(散文件跑法下 hook 照样装上,只有那个入口文件自身不被 instrument,见 §3),抗造得多。另一层收益是**命名即定位**(§7.9):`from myproj.retrieval.reranking import Reranker` 自带完整坐标,AI 和人一眼就知道这个符号来自哪个包的哪里;`from ..ports.protocols import LLM` 则必须先知道"我在哪"才能算出"它在哪"。由 `check_conformance.py` 机械检查。包外(`scripts/`、`tests/`、notebooks)本来就不是包,同样只能绝对导入(§7.8)。
- **找文件**与 import 无关,是另一件事:包内自带资源用 `importlib.resources` / `PackageLoader`;根级内容(configs/data)走 settings + 环境变量。根锚点由 `core/settings.py` 的公开常量 **`ROOT_DIR`** 统一提供(见附录),全项目一律 `from myproj.core.settings import ROOT_DIR`,不要在别处再算一次。

- **根锚点用仓库根标记文件 `.project-root`,不用 `pyproject.toml`。** 这**不是**把 rootutils 请回来——上一条仍然成立:不装 rootutils、不写 `sys.path`,`.project-root` 只是个用 `is_file()` 判一下**存在性**的惰性标记(文件里写着一行"请勿删除"的说明,内容从不被读取),与 import 解析毫无关系。选它的理由是 `pyproject.toml` 定位不准:monorepo 下每个子包都有一个,"向上找第一个"会停在**包根**而不是**仓库根**;`.project-root` 只放在仓库根,语义唯一。
  - 从 **`Path.cwd()`** 向上找(不是从 `__file__`),`cwd` 自身也参与匹配——在仓库根运行时锚就在当前目录。代价是**必须在项目树内运行**;树外运行(wheel/容器)用环境变量兜底。
  - **找不到就抛 `RuntimeError`,绝不静默退回 `Path.cwd()`**:静默退回会让 `ROOT_DIR` 变成"你碰巧在哪",`DATA_DIR`/`LOG_DIR`/yaml 路径跟着一起错,而且错得无声。宁可启动即炸。
  - 逃生口是环境变量 **`APP_ROOT_DIR`**:设了就直接用它,跳过向上查找。它**必须指向一个已存在的目录**,否则启动即 `RuntimeError`——路径写错而不校验,后果是 `configs/settings.yaml` 读不到、`Settings` 静默退回一堆默认值、`data/`/`logs/` 写去别处,全程无声。但它**不要求含 `.project-root`**:wheel/容器部署里根本没有那个标记文件(它只在仓库根、不进 wheel),那正是这个逃生口存在的意义。注意它**不是 `Settings` 的字段**——`ROOT_DIR` 在 `Settings` 实例化之前就要用到(`yaml_file` / `data_dir` / `log_dir` 的默认值都依赖它),所以只能由 `settings.py` 直接 `os.getenv` 读;`APP_` 前缀只是与 `Settings` 的 `env_prefix` 视觉一致,不代表它走 pydantic-settings 那条链路。
  - `.project-root` 在仓库根,而 wheel 只打 `src/<pkg>`,所以它**不进 wheel**——这是预期的:wheel 部署本就该用 `APP_ROOT_DIR` 显式指定,不要试图把标记文件塞进包里。

**开发期怎么跑(editable 安装是一次性的)**

editable 安装不拷贝代码,而是让 `import myproj` 直接指向你的 `src/myproj/` 源码。**装一次之后,改 src/ 里的代码下次运行立刻生效,不用重装**;提示词同理(`PackageLoader` 在 editable 下也指向真实源码目录,Jinja2 的 `auto_reload` 默认开着,改了即时生效——见 §7.6,这跟 `cache_size` 无关)。

用 uv,日常根本不用手动"安装":

```bash
uv sync                       # 一次性:建环境 + editable 安装本项目
uv run python -m myproj.cli   # 直接跑实时源码
uv run pytest                 # 跑测试,同样实时
```

**只有改了依赖(`uv add` 某个库)才需要重新同步;改自己的代码永远不用。**

### 7.2 可复用代码全进 src/

- 任何可复用的**领域逻辑**都放 `src/<pkg>/`,**哪怕只被 `scripts/` 或 `tests/` 调用**。
- 这和 beartype 协同:claw 只 instrument 包本身,`scripts/`、`tests/` 在包外不被自动检查;逻辑进 src/ 才会被运行时类型检查覆盖,留在脚本里就是盲区。
- **唯一例外**:测试专属脚手架(fixtures、mock 工厂、`conftest.py`)留在 `tests/`。判据:可复用领域逻辑 → src/;只为测试存在的基础设施 → tests/。

### 7.3 文件放哪:一条原则

- **随代码出厂、只读、每个部署都一样 → 包内**:`prompts/`(§7.5),用 `PackageLoader` / `importlib.resources` 定位,自动进 wheel,dev/prod 一致。
- **环境相关、可覆盖、可能含密钥 → 根级 + env**:`configs/settings.yaml`(§7.4),不进 wheel。
- **运行期可写 → 根级 + settings**:`data/`、`logs/`,路径来自 settings(可被 `APP_*_DIR` 覆盖)。

### 7.4 core/:跨切面基础设施的唯一来源

通用东西一处定义、处处调用,避免重复甚至出错。放 `core/`:

- **`settings.py`** —— pydantic-settings `Settings`,环境变量 + `.env` + `configs/*.yaml` 的统一入口;同时是全局常量与路径的唯一来源:根锚点 `ROOT_DIR`、运行期可写目录 `DATA_DIR`/`LOG_DIR`(从 settings 取),以及包内自带资源定位 `resource_path()`(用 `importlib.resources`)。**必须是 beartype 叶子**(§3)——它只依赖标准库(`pathlib`、`importlib.resources`)+ pydantic,叶子约束禁的是 import 一方模块,标准库/第三方不受限。

  **配置有两个出口,别用混:`get_settings()`(运行期)与 `settings`(import 期)。**
  只导出一个 import 期冻结的模块级单例,是一条**看不见的可测性断裂**:`Settings` 实例在
  第一次 import 时就构造完了,测试里 `monkeypatch.setenv("APP_RETRIEVER__TOP_K", "99")`
  之后再读它,拿到的仍是旧值——于是"换 provider""换配置"这一整类分支**一条都测不到**,
  而覆盖率报告不会告诉你这件事(那些行确实被执行过,只是永远走同一个分支)。
  所以:
  - `get_settings()` —— `@lru_cache(maxsize=1)` 的访问器,**运行期读配置一律走它**
    (`ports/factory.py`、`core/logging.py` 都是)。要换配置就设好环境变量再
    `get_settings.cache_clear()`,下次调用按新环境重建。
  - `settings` —— 保留的模块级单例,**只给 import 期用**:包 `__init__.py` 的 hook 要在
    import 期读 `settings.beartype_on`,那时既不可能也不需要覆盖配置。
  - 测试侧的用法是 `tests/conftest.py` 里的 `override_settings` fixture(§5),
    模板 `tests/test_settings.py` 是它的验收用例。
- **`logging.py`** —— 定义 `setup_logging()`(唯一来源)。**库代码只 `logging.getLogger(__name__)`**(已由 ruff `T20` 门禁强制:`print` 直接红),绝不在别处配置日志;`setup_logging()` 由入口启动时调用一次,幂等。同时放血缘与隐私两件工具:`log_provenance()`(每条 AI 产物记来源/实现/版本/计数)与 `SENSITIVE_FIELDS`(禁记字段清单),见 §10。
- **`prompts.py`** —— 提示词加载轮子(§7.5)。
- **`core/__init__.py` 保持空**(§3 铁律 3)。

### 7.5 配置:configs/ 经 pydantic-settings 并入 Settings(替代 Hydra)

复杂的全局配置外置成 `configs/settings.yaml`,由 `core/settings.py` 通过 **`YamlConfigSettingsSource`** 并入同一个 `Settings` 对象——**这就是 Hydra 配置文件的类型化替代**:你拿到文件式结构化配置,却不引入 Hydra `DictConfig` 那个 `Any` 黑洞。

- 复杂全局变量用**嵌套 pydantic 模型**表达(如 `retriever: RetrieverConfig`),在 yaml 里写嵌套结构。
- 优先级从高到低:**构造参数 > 环境变量 > `.env` > yaml > secrets**。即 yaml 当结构化基线,环境变量按部署覆盖;`APP_RETRIEVER__TOP_K` 可逐项覆盖嵌套字段。末位的 secrets 源(`file_secret_settings`)**只有在 `model_config` 设了 `secrets_dir` 时才读文件**;模板没设,所以它当前是 **no-op**——留在链尾是为了"要用 Docker secrets 时只加一行 `secrets_dir`",不是它现在在起作用。
- configs/ 是**环境相关的配置值**,不进 wheel(部署各异、可能含密钥),与"随代码出厂"的 prompts 正相反。

> 若日后需要重度实验扫参,再考虑 `tyro`/`jsonargparse` 或仅在实验层引入 Hydra;app/密钥始终走 pydantic-settings。

### 7.6 prompts/:包内提示词 + 统一加载轮子

- 提示词放**包内** `src/<pkg>/prompts/`(`prompts/<name>.md`,可带子目录),由 `core/prompts.py` 用 **`jinja2.PackageLoader`** 加载,其他代码 `from myproj.core.prompts import render_prompt` 调用。
- **一劳永逸**:`PackageLoader` 按包名定位,不依赖 CWD/项目根;包内 `.md` 用 hatchling 会**自动**打进 wheel(无需配 package_data);dev(editable)与生产(wheel)行为完全一致,没有"以后打包再搬"的分支。
- 用 Jinja2 + **`StrictUndefined`**:缺模板变量**直接报错**而非静默空串,契合"不静默失败"。
- **"改了提示词即时生效"来自 Jinja2 的 `auto_reload`(默认 `True`:模板 mtime 变了就重新编译),不是 `cache_size`。** 样板里那句 `cache_size=0 if settings.is_debug else 400` 只影响重复渲染的开销;而 `is_debug` 默认 `False`,所以默认跑的其实是"400 条缓存 + auto_reload"这一支——**照样即时生效**。别把功劳记在没干活的旋钮上:真要关掉热更新,得显式设 `auto_reload=False`(生产想省那次 stat 时才有意义)。
- 取舍:提示词从此**随代码版本走**,改提示词 = 改仓库 + 重新部署(更规范);要运行时热换提示词是另一层功能(外部提示词存储),不在此。

### 7.7 目录树

下面就是 `scripts/scaffold.py` 实际生成的东西(标 *可选* 的除外),不是一棵理想化的示意树:

```
repo/
├── configs/
│   └── settings.yaml        # 环境相关配置值;不进 wheel(由 settings 的 YAML 源读取)
├── data/  logs/             # 运行期可写产物;.gitignore 忽略内容、保留 .gitkeep
├── docs/
│   └── adr/                 # 编号、不可变的架构决策记录(§12);先决策再编码
│       └── 0001-record-architecture-decisions.md
├── notebooks/               # 可选,scaffold 不生成;只做探索,别攒可复用逻辑(§7.8)
├── scripts/                 # 薄:只编排,逻辑全在 src/
│   ├── check_conformance.py #   结构不变量检查器(scaffold 从 skill 复制进来)
│   └── check_drift.py       #   文档漂移守卫(covers: 元数据)
├── src/
│   └── myproj/              # ← 真实包名(自用也别叫 src);上架就是发行名
│       ├── __init__.py      # 唯一安装 beartype claw hook 的地方;只放 hook 与 re-export
│       ├── prompts/         # 提示词随包出厂(PackageLoader 加载,dev/prod 一致)
│       │   └── rag/
│       │       └── answer.md
│       ├── core/
│       │   ├── __init__.py   #   必须为空(否则 logging/prompts 会在 hook 前被导入)
│       │   ├── settings.py   #   Settings(env + .env + configs/*.yaml)+ ROOT_DIR/DATA_DIR/LOG_DIR + resource_path() —— 叶子模块
│       │   ├── logging.py    #   setup_logging() / log_provenance() / SENSITIVE_FIELDS
│       │   └── prompts.py    #   提示词加载轮子:render_prompt / get_prompt
│       ├── ports/           # 模型无关的缝:核心与领域代码只依赖这里(§9)
│       │   ├── protocols.py  #   每个外部 AI 依赖一个最小 Protocol
│       │   ├── factory.py    #   唯一装配缝:按 settings 选实现,未知配置显式抛错
│       │   └── exceptions.py #   ProviderError:厂商/网络错在 adapter 边界归一到它
│       ├── adapters/        # Protocol 的实现;**唯一允许 import 厂商 SDK 的地方**(lazy)
│       │   ├── mock.py       #   MockProvider 作 default(不是测试桩):离线可跑
│       │   └── openai_llm.py #   示例真实 adapter
│       ├── schemas/         # 跨领域共享的 pydantic 契约(领域私有的放该领域 models.py)
│       ├── <domain>/        # 业务模块:retrieval / ingestion / generation ...
│       │   ├── __init__.py   #   hook 之后导入,可 thin re-export 公共 API
│       │   └── README.md     #   就近契约(带 covers: frontmatter,受漂移守卫保护)
│       └── cli.py           # 入口:启动时调 setup_logging() 一次;console script 挂它
├── tests/
│   ├── conftest.py          # 测试专属 fixtures(含 override_settings)—— 不进 src/
│   ├── test_smoke.py        #   无 SDK、无网络也能跑通主链路(§9 的硬性验收)
│   ├── test_conformance.py  #   一致性契约:所有实现跑同一组不变量
│   └── test_settings.py     #   配置缝的验收:换 provider / 覆盖嵌套字段
├── .project-root            # 仓库根标记;ROOT_DIR 从 CWD 向上找它,别删
├── .env.example
├── .gitignore
├── ci.sh                    # 唯一的零警告门(必须在仓库根跑);make hooks 可钉进 pre-push
├── Makefile                 # check / dev-test / fmt / hooks 的薄封装
├── CLAUDE.md                # 根级路由表:硬约束 + 去哪找(§11)
├── pyproject.toml
└── README.md
```

**没有 `.pre-commit-config.yaml`,这是刻意的。** 门只能有一条:再引一个 pre-commit 框架,
就等于让"该跑哪些检查"存在第二份清单,两边各自漂移;它的 mirrors 还会各自钉一套
ruff/mypy 版本,与 `[dependency-groups]` 里的版本对不上,于是"本地绿、CI 红"。
要把门钉在推送前,`make hooks` 装一个 `exec ./ci.sh` 的 git `pre-push` 钩子即可,
零新增依赖、零第二份清单。

### 7.8 调用约定:scripts / notebooks / tests 里用 src/

三者都在包外,而 `myproj` 是 editable 安装的,所以**统一像用第三方库一样** `from myproj.X import Y`——零 sys.path 操作、零 rootutils。导入 `myproj.*` 会自动触发 §3 的 hook,你调用的 src/ 代码因此被运行时检查(策略按环境);它们**自身**的代码在包外、不被 instrument(可复用逻辑本就该在 src/,见 §7.2)。

前置:`uv sync` 一次(editable 安装),之后改 src/ 即时生效。

**tests/**

```bash
uv run pytest            # 必须在仓库根跑
```

- **`pytest` 和 `./ci.sh` 一律从仓库根跑。** 在深子目录里跑(如 `cd src/myproj && pytest`)会收集到 **0 个用例**、只打一行 "no tests ran" 就结束(exit 5)——看着不像红,实则什么都没测,是这套流程里最容易骗过 agent 的失效模式。`tests/conftest.py` 里有一条守卫:CWD 不含 `.project-root`(且未设 `APP_ROOT_DIR`)就 `pytest.UsageError` 当场失败。但要注意它**只在 conftest 被加载时生效**——从 `src/` 深处跑时 tests/ 根本没被收集,守卫也不会加载,所以这条纪律仍需人和 agent 自己守:**永远在仓库根跑门**。
- `from myproj.pipeline import run` 直接导入(src 布局 + editable 让"测试导不到包"的经典问题自动消失)。
- 用 `--import-mode=importlib`(已在 pyproject),**tests/ 不需要 `__init__.py`**。
- 测试专属 fixtures 放 `conftest.py`,不进 src/。
- beartype 违例在测试里会变成测试失败——这正是它帮你抓 bug 的方式(强度见 §3:O(1) 抽样)。

**scripts/**

```bash
uv run python scripts/backfill.py
```

- 只用**绝对导入** `from myproj.X import Y`(scripts/ 不是包,别用相对导入)。
- 脚本是入口层:开头调一次 `setup_logging()`(§7.4 的日志纪律)。
- 跑 scripts/ 里的散文件 OK(它一 import myproj 就装上 hook);但**包内**模块要用 `python -m myproj.cli`,别 `python src/myproj/cli.py`(§3)。

**notebooks/**

```bash
uv run jupyter lab       # kernel 跑在装了 myproj 的环境里
```

第一格开 autoreload,改 src/ 不用重启 kernel:

```python
%load_ext autoreload
%autoreload 2
from myproj.core.logging import setup_logging
setup_logging()
```

之后 `from myproj.X import Y` 正常用。notebook 只做探索/可视化,**别在里面攒可复用逻辑**;值得留的代码重构进 src/。

### 7.9 结构的可导航性:domain-first 分层

**命名即路径**——让 agent 由名字直接定位代码,是结构层面的"机器友好",与类型在接口层面的作用对应。"修 reranker"应当无需搜索就能推断到 `retrieval/reranking/`。

- **domain-first,不 layer-first**:按能力/领域分包(`retrieval/`、`ingestion/`),不要按技术种类平铺(所有模型塞 `models/`、所有杂项塞 `utils/`)。前者随规模扩展、名字直接指向位置。
- **可嵌套到 3–4 层**:`src/<pkg>/<domain>/<subdomain>/<module>.py`。别为深而深——叶子模块要有实质内容,避免每层只有一个文件的"洋葱"。
- **叶子包形状一致**:每个领域叶子包可含 `models.py`(该领域的 pydantic 契约)、实现模块、`__init__.py`(thin re-export 公共 API)。这样 agent 在任何包内都知道去哪找。
- **数据契约的位置**:跨领域共享的 pydantic 模型放顶层 `schemas/`;领域私有的放该领域包内 `models.py`。
- **`core/` 只放基础设施**(settings/logging/prompts),不掺业务。
- **__init__ 与 beartype**:深化后**只有 `core/__init__.py`(以及从顶层 `__init__.py` 到 settings 路径上的任何包)必须为空**;其余子包 `__init__.py` 在 hook 之后导入,可以 re-export 公共 API——这反而**助于导航**(`from myproj.retrieval import Reranker`)。

示例(RAG/agent,按需裁剪):

```
src/myproj/
├── __init__.py              # beartype hook
├── core/                    # 跨切面基础设施(非业务)
│   ├── __init__.py          #   空
│   ├── settings.py  logging.py  prompts.py
├── ports/                   # 模型无关的缝:领域代码只依赖这里,零 SDK import(§9)
│   ├── protocols.py  factory.py  exceptions.py
├── adapters/                # Protocol 的实现;唯一允许 import 厂商 SDK 的地方(lazy)
│   ├── mock.py  openai_llm.py
├── prompts/                 # 提示词资源(随包出厂)
│   └── rag/answer.md
├── schemas/                 # 跨领域共享的 pydantic 数据契约
│   ├── documents.py  retrieval.py
├── ingestion/               # 领域:文档摄入
│   ├── parsing/  chunking/  embedding/
├── retrieval/               # 领域:检索
│   ├── vector/  reranking/  fusion/
├── generation/              # 领域:生成
│   ├── llm/  synthesis/
├── agents/                  # 领域:agent 编排
│   ├── tools/  orchestration/
├── pipelines/               # 端到端管道组装
├── api/                     # 对外接口(FastAPI 等)
└── cli.py                   # 入口
```

`ports/` 与 `adapters/` 是**并列于领域的两个顶层包,不是某个领域的下属**:每个领域都可能
经同一个缝取 LLM/embedder,把缝塞进任一领域包都会让别的领域反向依赖它。
`check_conformance.py` 的封闭式白名单也正是按 `adapters/` 这个路径豁免的(§9)。

---

## 8. Agent 工作流

```
写/改代码
  → 跑 §1 的单条命令
  → 解析机器可读错误(ruff / mypy / pytest+beartype)
  → 定位修复
  → 重复直到全绿
  → 提交
```

- 该命令是 agent 唯一的正确性判据,**必须在仓库根跑**(§7.8)。
- 深度靠 Hypothesis:beartype 只做 O(1) 抽样(§3),大量随机输入 × 每次调用都抽一下,才是把"抽样"变成"覆盖"的正确杠杆。
- 多 agent 协作时质量门一致;任何 agent 的产出都要过这条命令才算完成。

---

## 9. 模型无关:把每个 AI 依赖收敛成一个可热插拔的缝

模型是可热插拔的商品。一旦正确性依赖某个具体模型的输出,你就和它联姻——把正确性、可测性、可审计性从模型里抽出来,钉死在确定性代码上。

- **每个外部依赖 = 一个最小 Protocol(`ports/protocols.py`)。** LLM/embedding/reranker/向量库/解析器各一个窄接口;核心与领域代码只依赖 `ports/`,**import 厂商 SDK 数为 0**。这条由 `check_conformance.py` 用 **AST** 机械强制,而且是**封闭式**的:`src/<pkg>/` 里 `adapters/` 之外的模块,其 import 的顶层包必须落在「标准库 + 本包自身 + `pyproject` 的 `[project.dependencies]` 运行时依赖」之内。**不是厂商黑名单**——黑名单永远漏下一个新 SDK;封闭式默认拒绝,厂商 SDK 待在 `[project.optional-dependencies]` 里就自动不在白名单内,谁也不用去登记。接口越窄能塞的实现越多,"换模型"从重构降级为加一个类。
- **唯一装配缝(`ports/factory.py`)按 env 选实现并注入。** 业务代码不写裸厂商名,只调 `make_llm()`;未知配置显式抛错。
- **SDK 只在 `adapters/`,且 lazy import,厂商异常归一到 `ProviderError`。** 厂商包进可选 extras;网络/超时/API 错包成自有边界异常,程序错(KeyError/TypeError)照常上抛——**严禁 `except Exception` 把所有错塞进降级路径**。这条已由 ruff `BLE` 门禁强制(此前它只是一句散文,写了也不会红)。
- **MockProvider 作 default(不是测试桩)。** 不装 SDK、不连网也能跑通主链路并通过测试;CI 快、稳、免费、不被随机性污染。"无 key 也能 demo/test 跑绿"设成硬性验收(`tests/test_smoke.py`)。
- **一致性契约(conformance kit,`tests/test_conformance.py`)把所有实现绑死在同组不变量上。** 任何号称实现了某 Protocol 的类(Mock 与真实后端)都跑同一组行为契约——可插拔只在"所有插头行为一致"时才安全,否则 bug 以"换了模型后偶发"出现。

## 10. 让 AI 输出可信:把约束下沉到控制流,而非写进 Prompt

Prompt 是软约束,温度/越狱/长上下文都能绕过;只有沉到代码的约束,才能从"概率性遵守"变成"结构上不可能违反"。本节偏原则(不像结构那样能完全机械检验),但对 AI-touching 代码是上位纪律。

- **Constrain, don't ask。** 对不可妥协的属性(不编造、必引用、不越权),让模型物理上无法违反:命中事实时答案由代码从结构化值确定性合成、模型散文整段丢弃;无事实时编排层改写成"查不到"。迁移:列"绝不能发生"清单,逐条问"模型能否在听话的同时仍违反它",凡"能"的就移出模型。
- **收窄发射面。** 不让模型自由生成关键载荷:让它在受控选项里选、调用返回三态(found/not_found/unrecognized)的工具,最终数字一律取自工具结果而非模型文本。模型能犯的错与它能输出的"面积"成正比。
- **安全门确定性、独立、永不可插拔。** 理解类(意图解析)可插拔;安全决策(越权/敏感隔离)做成确定性代码门,从原始输入独立重判,不信任可插拔组件的输出。可替换的是智能,不是护栏。
- **血缘进、隐私出(`core.logging.log_provenance` + `SENSITIVE_FIELDS`)。** 每条产出带来源 + 产出它的实现/版本号(多实现可并存可审计);trace 只记码值/计数/耗时,绝不落答案/数值/原文。

## 11. 把人类的隐性兜底外化成会失败的工件

人靠经验、记忆、羞耻感判断"做完没""文档过时没";agent 没有这些,只优化能观测到的反馈。每加一个会失败的检查,就把一份隐性知识变成 agent 无法悄悄绕过的硬约束。

- **加宽的零警告门(`ci.sh`)。** 一条 `set -euo pipefail` 脚本串起:ruff + mypy `--strict` + **结构检查(`scripts/check_conformance.py`)** + 文档漂移 + beartype 测试 + 冒烟 + **warnings-as-errors**(pytest `filterwarnings=["error"]`);任一项非零即没完成。人和 agent 共用同一条判据(**在仓库根跑**),用 pre-push hook 钉死。
- **结构检查器必须随项目出厂并挂进门。** `scripts/check_conformance.py` 是 stdlib-only、接受项目根参数的独立脚本,scaffold 时复制进项目 `scripts/`(skill 里那份是源,项目里那份是快照,与其它模板文件同一种关系)。它用 **AST** 而不是字符串匹配:被注释掉的 `beartype_this_package()` 能骗过 grep,而"hook 被注释掉"恰恰是它最该抓的失效。
- **漂移守卫(`scripts/check_drift.py`)。** 描述代码的文档带 `covers:` 元数据,被覆盖的路径失效即 CI 红——AI 开发最大的隐性成本是"上下文骗了它",给输入端装真实性保险丝。可扩展到符号级反射(改名/删除即红)。

  **`covers:` 语法**(就是检查器实际认的那套,别写它读不懂的形式):在 `.md` 里任取一行以 `covers:` 开头(通常放 YAML frontmatter 里),值是逗号分隔的**仓库根相对路径**,方括号和引号可写可不写;文件和目录都行,只查**存在性**。

  ```markdown
  ---
  covers: [src/myproj/retrieval/__init__.py, src/myproj/retrieval/reranking]
  ---
  ```

  等价的单条写法是 `covers: docs/adr`。检查器扫全仓库的 `*.md`(跳过 `.git`/`.venv`/`node_modules` 等,否则**依赖树里任何一个带 `covers:` 的包 README 都会让你假红**),逐条 `(ROOT / path).exists()`,有失效即列出并退出 1。scaffold 生成的每个 `src/<pkg>/<domain>/README.md` 都自带一份可用的 frontmatter 作范例。
- **本规范自己的工具脚本也受同一条门约束(`python-project-standard/ci.sh`)。** 在 skill 目录跑 `./ci.sh`,对 `scripts/`(`scaffold.py` / `check_conformance.py` / `check_appendix_sync.py`)执行 `ruff format --check` + `ruff check` + `mypy --strict`。规则不另立一份——`--config` / `--config-file` 直接指向 `assets/templates/pyproject.toml`,即被 scaffold 出去的项目所用的同一份配置,两边不可能各自漂移。工具经 `uvx` 拉隔离副本,干净机器免预装,也不会误用全局/Anaconda 那份。教一套做另一套是规范腐烂的起点,所以这件事交给机器而不是自觉。
- **文档里贴的样板代码也要机械对齐(`scripts/check_appendix_sync.py`,挂在同一条门里)。** 本文附录的代码块必须与 `assets/templates/` 下**发货的那一份逐字一致**(比对时把 `__PACKAGE_NAME__` 归一成 `myproj`),不一致就退出 1 并指出是哪一块、差在第几行。这是"文档漂移"里最贵的一类:两处各自演进后,照着附录写的人和 AI 会按一份**已经不存在**的代码干活,而它不会让任何测试变红。实测这类漂移在本仓库已发生过两次(附录的 `logging.py` 缺了 §10 正在引用的 `SENSITIVE_FIELDS` / `log_provenance()`),所以别靠"记得同步",靠机器。
- **自描述工件从真实代码派生。** 架构/拓扑图从真实接线内省派生而非手画;手维护的图必然腐烂,而腐烂的图比没有更危险——会自信地误导后续 agent。
- **分层上下文,而非一份大文档读到底(`CLAUDE.md`)。** 根级只放"路由表"(硬约束 + 去哪找)、领域子树放就近的简短契约、深挖文档靠 grep、生成物单独成区并 git-ignore。上下文窗口稀缺,"什么都塞"稀释信号。

## 12. 驾驭 AI 做大工作:可拆分、可逐步验证、可对抗审查

把 AI 当可监督的劳力,而非无监督的自动驾驶。这是工作流层(可作姊妹技能),轻量版规则:

- **先决策再编码(ADR,`docs/adr/`)。** 架构/产品决策写成编号、不可变的 ADR(背景 + 选定 + 被否决备选及理由),AI 只在锁定边界内填空。AI 最擅长明确约束下填实现,最不可靠的是替你做含糊取舍;否决理由还防它重走已排除的路。
- **TDD 红灯先行,测试即不可动摇的规格。** 每个改动先写会失败的测试再实现到绿,**绝不为通过而弱化测试**;给 subagent 的任务直接附上失败测试作为验收。
- **拆成编号步骤、每步独立绿灯。** 大重构拆成有序步骤,每步一次提交、跑完整门禁再进入下一步,绝不攒巨型 diff——把失败爆炸半径压到一步之内。
- **分解→并行→综合,主审者把关每个 diff。** 主 agent 拆出边界清晰、规格完整的子任务并行分派,逐个审查 diff 并在提交前跑门禁。
- **对抗式独立复审:用怀疑的 AI 查 AI。** 写完后另起独立、带敌意的多视角复审(明确"假设它有错,去证伪"),优先审无法被测试覆盖的产物(图、文档、取舍)——同一 agent 既写又夸会系统性确认偏差。

---

## 附录:配置与样板

> **本附录的每个代码块都与 `assets/templates/` 下发货的那一份逐字一致,并由 skill 自检门里的
> `scripts/check_appendix_sync.py` 机械保证**(比对时把 `__PACKAGE_NAME__` 归一成 `myproj`)。
> 改了模板不改这里(或反过来)会让 skill 的 `./ci.sh` 直接红。
> 附录只收**核心基础设施模块**;`ports/`、`adapters/`、`cli.py` 与 `tests/` 只在模板目录里。

> 依赖:`uv add pydantic "pydantic-settings[yaml]" beartype jinja2`;`uv add --dev mypy ruff pytest hypothesis openai`(`openai` 进 dev 组是为了让 mypy 能真类型检查示例 adapter,与模板一致;运行时它仍在 extras 里)。
> v2 里 `BaseSettings` 在独立的 **pydantic-settings** 包;`[yaml]` extra 引入 PyYAML 以支持 `configs/*.yaml`。
> 确保 beartype / pydantic 为支持 **Python 3.13** 的版本。
> 一次性安装本项目自身(editable):`uv sync`;之后改 src/ 即时生效,只有改依赖才需重新同步。

**开发工具进 `[dependency-groups]`,厂商 SDK 进 `[project.optional-dependencies]`**——这两处别混。
`dev` 是开发环境的工具链,不是本包对外的可选运行时特性:放 extras 会逼所有人 `uv sync --extra dev`,
少打一次参数门就跑不起来;放 PEP 735 的 `dependency-groups`,**裸 `uv sync` 默认就装 `dev` 组**,
`./ci.sh` 开箱即绿,而这些工具也不会泄进下游安装本包的用户。反过来,`openai` 这类厂商 SDK
是真正的可选运行时特性(选了该 provider 才需要),必须留在 extras 里。

### `pyproject.toml`

```toml
[project]
name = "myproj"
version = "0.1.0"
description = "按 python-project-standard 生成的项目。"
readme = "README.md"
requires-python = ">=3.13"
dependencies = ["pydantic", "pydantic-settings[yaml]", "beartype", "jinja2"]

[project.scripts]
# console script:`uv sync` 后可直接 `uv run myproj`。
# 这是「从包进入」的正规入口之一(见 standard.md §3),hook 必装上。
myproj = "myproj.cli:main"

[project.optional-dependencies]
# 厂商 SDK 进可选 extras:真正的可选运行时特性,核心/测试不依赖,装了才用
openai = ["openai"]

[dependency-groups]
# 开发工具不是本包的运行时特性,进 PEP 735 组:裸 `uv sync` 默认装 dev 组
# openai 在此出现是为了让 mypy 能真类型检查 adapters/ 里的示例 adapter;
# 运行时依旧只有选了该 provider 才需要(见 optional-dependencies)。
dev = ["mypy", "ruff", "pytest", "hypothesis", "openai"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[tool.hatch.build.targets.wheel]
packages = ["src/myproj"]
# hatchling 默认把包目录下的非 .py 文件(如 prompts/*.md)一并打进 wheel

[tool.mypy]
python_version = "3.13"
strict = true
warn_unreachable = true
# 裸 `# type: ignore` 直接报错:必须写成 `# type: ignore[code]  # 理由`,
# 否则一条 ignore 会顺手关掉同一行上所有别的错误,而且没人知道它当初在压什么。
enable_error_code = ["ignore-without-code"]

[tool.ruff]
target-version = "py313"
src = ["src", "tests"]

[tool.ruff.lint]
# BLE:禁 `except Exception` 兜底(standard.md §9 的「严禁」,不能只写在文档里)
# PGH:blanket-type-ignore / blanket-noqa,补 ruff 侧的「压制必须带错误码」
# T20:禁 print,兑现「库代码只 getLogger」(§7.4);例外见 per-file-ignores
select = ["E", "F", "I", "UP", "ANN", "B", "SIM", "RUF", "TID", "BLE", "PGH", "T20"]

[tool.ruff.lint.flake8-tidy-imports]
# 包内禁相对导入(§7.1)。check_conformance.py 也查这条,但 ruff 反馈更快、
# 且覆盖 tests/ 与 scripts/(那两处在包外,结构检查器不扫)。
ban-relative-imports = "all"

[tool.ruff.lint.flake8-tidy-imports.banned-api]
# PEP 563 是被官方放弃的过渡路径,注解一律保持「活对象」(理由见 standard.md §6)
"__future__.annotations".msg = "本规范禁用 from __future__ import annotations:注解保持活对象,前向/自引用类型加引号即可。"

[tool.ruff.lint.per-file-ignores]
# scripts/ 是命令行工具层,不是库代码:门检查器的输出契约就是 stdout
# (人可读报告 + 退出码),经 logging 转一手只会让它更难用。
# 这是刻意的、按目录划定的豁免,不是逐行 noqa;src/ 里的 T20 依旧全额生效。
"scripts/*.py" = ["T20"]

[tool.pytest.ini_options]
addopts = "-q --import-mode=importlib"
testpaths = ["tests"]
filterwarnings = ["error"]   # warnings-as-errors;已知噪声依赖再逐条 ignore
```

`version` 不是可选的:`[project]` 表在场时必须有 `version`(或把它列进 `project.dynamic`),
否则 **`uv sync` 直接失败**。报错的是 **uv 自己**,在解析 `pyproject.toml` 这一步就停了
(`` `pyproject.toml` is using the `[project]` table, but the required `project.version`
field is neither set nor present in the `project.dynamic` list``)——根本轮不到 hatchling
被调用,所以别去 build backend 那边找原因。`target-version` 必须与 `requires-python`
的下界、`[tool.mypy] python_version` 三者一致——ruff 要按**最低支持版本**判 `UP` 类规则,
写高了会建议你用低版本跑不了的语法。

### `src/myproj/__init__.py`

```python
"""包初始化:在导入任何子模块之前,按配置安装 beartype 运行时类型检查。

claw hook 只对其安装之后导入的模块生效,所以本文件必须最先执行;
hook 之前只能导入 settings(叶子)——不要导入任何想被检查的一方模块。

本文件只放 hook 与 re-export,**不要在此定义函数**:hook 安装时本文件已在执行中,
其中定义的函数永远不会被 instrument。

策略固定为 beartype 默认的 O(1) 抽样(容器只随机抽一个元素检查):
BeartypeStrategy.On(线性全量)在 beartype 里至今未实现(官方源码标注
"currently unimplemented"),与 O1 生成的 wrapper 完全相同,分档只是自欺。
容器深处的坏数据由边界处的 pydantic 校验兜底(见规范 §4)。
"""

# 叶子;有意在 hook 之前导入(本身不被检查)
from myproj.core.settings import settings

if settings.beartype_on:
    from beartype.claw import beartype_this_package

    beartype_this_package()

# ↓ 其它包级导入/导出一律放在 hook 之后
```

> **顶层 `__init__.py` 里定义的函数永远不被检查。** hook 是在本文件执行到一半时才装上的,此刻本文件自身早已在解释器里跑着——它里面的 `def` 不会被 instrument(和"散文件跑法下入口文件自身漏检"是同一个成因)。所以顶层 `__init__.py` **只放 hook 和 re-export**,任何函数都挪进子模块。`check_conformance.py` 会机械查这条。

### `src/myproj/core/__init__.py`

```python
# 必须保持空。
# 导入 settings(叶子)会先经过本文件;若在此 re-export logging / prompts,
# 它们会在 beartype claw hook 安装前被导入,从而漏掉运行时类型检查。
```

### `src/myproj/core/settings.py`

```python
"""全局配置、常量与路径的唯一来源:环境变量 + .env + configs/settings.yaml。

根锚点 ROOT_DIR、运行期可写目录 DATA_DIR / LOG_DIR、包内资源定位 resource_path()
全部在此定义;其他模块一律从这里导入,不要自己再算一次项目根。

beartype 叶子约束:本模块不得 import 任何本项目内、希望被检查的模块。
只依赖标准库 + pydantic / pydantic-settings(第三方依赖不受叶子约束限制)。
"""

import os
from functools import lru_cache
from importlib.resources import files
from pathlib import Path

from pydantic import BaseModel
from pydantic_settings import (
    BaseSettings,
    PydanticBaseSettingsSource,
    SettingsConfigDict,
    YamlConfigSettingsSource,
)


def _find_project_root() -> Path:
    """从 CWD 逐级向上找标记文件 .project-root,定位仓库根。

    用专用标记而不是 pyproject.toml:monorepo 下每个子包都有 pyproject.toml,
    向上找会停在子包根;.project-root 只放在仓库根,"向上找第一个"天然停对地方。

    找不到即抛错,绝不静默退回 Path.cwd() —— 那会让 ROOT_DIR 变成"你碰巧在哪"。
    非源码部署(wheel / 容器)或需要在项目树外运行时,用 APP_ROOT_DIR 显式指定。
    注意 APP_ROOT_DIR 不是 Settings 的字段(ROOT_DIR 在 Settings 实例化之前就要用),
    只能在这里直接 os.getenv 读;前缀 APP_ 只是与 Settings 的 env_prefix 保持一致。

    APP_ROOT_DIR 只校验「存在且是目录」,**不要求它含 .project-root**:
    wheel / 容器部署里根本没有那个标记文件(它只在仓库根、不进 wheel),
    这正是这个逃生口存在的意义。但路径本身写错必须当场炸——否则 configs/ 读不到、
    data/ 与 logs/ 悄悄写去别处,配置静默退化成一堆默认值而没人发现。
    """
    if env_root := os.getenv("APP_ROOT_DIR"):
        root = Path(env_root).resolve()
        if not root.is_dir():
            raise RuntimeError(
                f"APP_ROOT_DIR={env_root!r} 指向的路径不存在或不是目录"
                f"(解析为 {root})。\n请把它设成一个已存在的目录的绝对路径"
                "——ROOT_DIR 由它决定,configs/ data/ logs/ 都相对它解析。"
            )
        return root
    cwd = Path.cwd()
    # cwd 自身必须参与匹配:在仓库根运行时,锚就在当前目录而不在 parents 里
    for parent in [cwd, *cwd.parents]:
        if (parent / ".project-root").is_file():
            return parent
    raise RuntimeError(
        f"未找到仓库根标记文件 .project-root(已从 {cwd} 逐级向上找到文件系统根)。\n"
        "请在项目树内运行,或设 APP_ROOT_DIR=<仓库根绝对路径> 显式指定根目录。"
    )


ROOT_DIR: Path = _find_project_root()


# —— 复杂全局变量用嵌套模型表达,yaml / env 都能填 ——
class RetrieverConfig(BaseModel):
    top_k: int = 5
    rerank_model: str = "cohere/rerank-v4.0-fast"


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="APP_",  # APP_IS_DEBUG、APP_BEARTYPE_ON ...
        env_nested_delimiter="__",  # APP_RETRIEVER__TOP_K 覆盖嵌套字段
        env_file=".env",
        env_file_encoding="utf-8",
        yaml_file=ROOT_DIR / "configs" / "settings.yaml",
        extra="ignore",
    )

    is_debug: bool = False  # 日志级别、prompt 缓存等(与 beartype 解耦)
    beartype_on: bool = True  # 运行时类型检查总开关;仅生产设 APP_BEARTYPE_ON=false

    # provider 选择(唯一装配缝按 env 切换;默认 mock,离线/CI 可跑)
    llm_provider: str = "mock"
    embedder_provider: str = "mock"

    # 运行期可写目录(默认锚定项目根;部署可用 APP_*_DIR 覆盖)
    data_dir: Path = ROOT_DIR / "data"
    log_dir: Path = ROOT_DIR / "logs"

    # 复杂结构化配置(来自 configs/settings.yaml,可被 env 覆盖)
    retriever: RetrieverConfig = RetrieverConfig()

    @classmethod
    def settings_customise_sources(
        cls,
        settings_cls: type[BaseSettings],
        init_settings: PydanticBaseSettingsSource,
        env_settings: PydanticBaseSettingsSource,
        dotenv_settings: PydanticBaseSettingsSource,
        file_secret_settings: PydanticBaseSettingsSource,
    ) -> tuple[PydanticBaseSettingsSource, ...]:
        # 优先级从高到低:构造参数 > 环境变量 > .env > yaml > secrets。
        # 末位的 file_secret_settings 只有在 model_config 设了 secrets_dir 时才读文件;
        # 本模板没设,所以它当前是 no-op —— 留在链尾是为了「要用 Docker secrets
        # 时只需加一行 secrets_dir」,而不是它现在在起作用。
        return (
            init_settings,
            env_settings,
            dotenv_settings,
            YamlConfigSettingsSource(settings_cls),
            file_secret_settings,
        )


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    """配置的访问器:进程级单例,但**可测**。

    运行期读配置的代码一律走这里,别捕获模块级的 `settings`——那个实例在 import
    期就冻结了,测试里 `monkeypatch.setenv(...)` 对它毫无作用,于是「不同配置 /
    不同 provider」这一整类分支根本测不到。

    测试要换配置:设好环境变量后 `get_settings.cache_clear()`,下次调用即按新环境
    重建(模板 tests/conftest.py 的 override_settings fixture 就是这么做的)。
    """
    return Settings()


settings = get_settings()
"""模块级单例:专供 import 期使用者。

包 `__init__.py` 的 beartype hook 在 import 期读 `settings.beartype_on`,那时既不
可能也不需要覆盖配置,用单例最省事。**运行期**的读取请改用 `get_settings()`。
"""

_PKG = __name__.split(".")[0]  # 顶层包名,重命名安全

DATA_DIR: Path = settings.data_dir
LOG_DIR: Path = settings.log_dir


def resource_path(relative: str) -> Path:
    """包内自带资源的路径(假设文件系统安装,如 Docker/服务器部署)。

    zip 安装场景请改用 importlib.resources.as_file 上下文管理器。
    """
    return Path(str(files(_PKG))) / relative
```

### `configs/settings.yaml`

```yaml
# 结构化全局配置;环境变量(APP_*)可逐项覆盖,如 APP_RETRIEVER__TOP_K=20
retriever:
  top_k: 10
  rerank_model: cohere/rerank-v4.0-fast
```

### `src/myproj/core/prompts.py`

```python
"""提示词加载轮子:从包内 prompts/ 读取与渲染,供其他代码调用。

提示词随包出厂(src/<pkg>/prompts/<name>.md),用 PackageLoader 定位,
dev(editable)与生产(wheel)行为一致,不依赖 CWD 或项目根。
约定:render_prompt("rag/answer", ...) 对应 <pkg>/prompts/rag/answer.md。
"""

from jinja2 import Environment, PackageLoader, StrictUndefined

from myproj.core.settings import settings

_PKG = __name__.split(".")[0]  # 顶层包名,重命名安全
_loader = PackageLoader(_PKG, "prompts")
_env = Environment(
    loader=_loader,
    undefined=StrictUndefined,  # 缺变量直接报错,而非静默空串
    autoescape=False,  # 提示词不是 HTML
    # 「改了提示词即时生效」来自 Jinja2 的 auto_reload(默认 True:模板 mtime 变了
    # 就重新编译),**不是**下面这个 cache_size。cache_size 只影响重复渲染的开销。
    # 注意 is_debug 默认 False,所以默认走的是「400 条缓存 + auto_reload」这一支,
    # 依旧即时生效。别把功劳记在没干活的旋钮上。
    cache_size=0 if settings.is_debug else 400,
)


def render_prompt(name: str, /, **variables: object) -> str:
    """渲染带 Jinja2 变量的提示词;缺变量会报错。"""
    return _env.get_template(f"{name}.md").render(**variables)


def get_prompt(name: str) -> str:
    """读取原始提示词文本(无变量场景)。"""
    source, _, _ = _loader.get_source(_env, f"{name}.md")
    return source
```

### `src/myproj/core/logging.py`

```python
"""日志配置的唯一来源 + AI 产物的血缘与隐私纪律。

纪律:
- 库代码(src/ 内)只 `logging.getLogger(__name__)`,绝不在本模块之外配置日志。
- `setup_logging()` 由入口在启动时调用一次,幂等。
- trace 只记**码值 / 计数 / 耗时**,绝不落答案 / 原文 / 向量值等敏感载荷。
  禁记字段清单见 SENSITIVE_FIELDS。
- 每个 AI 产物带来源 + 产出它的实现/版本号(多实现可并存可审计)——用 log_provenance。
"""

import logging
from logging.config import dictConfig

from myproj.core.settings import get_settings

# 禁记字段:这些一律不进 log(隐私 + 体积)
SENSITIVE_FIELDS = frozenset(
    {"answer", "text", "content", "prompt", "embedding", "value"}
)

_configured = False


def setup_logging() -> None:
    global _configured
    if _configured:
        return
    level = "DEBUG" if get_settings().is_debug else "INFO"
    dictConfig(
        {
            "version": 1,
            "disable_existing_loggers": False,
            "formatters": {
                "default": {"format": "%(asctime)s %(levelname)s %(name)s: %(message)s"}
            },
            "handlers": {
                "console": {"class": "logging.StreamHandler", "formatter": "default"}
            },
            "root": {"handlers": ["console"], "level": level},
        }
    )
    _configured = True


def log_provenance(
    source: str, impl: str, version: str, *, count: int | None = None
) -> None:
    """记录 AI 产物的来源元数据(来源 / 实现 / 版本 / 计数),绝不记录载荷本身。"""
    logging.getLogger("provenance").info(
        "source=%s impl=%s version=%s count=%s", source, impl, version, count
    )
```

### `Makefile`(或 justfile)

```makefile
check:                    # 完整门禁(唯一正确性判据);也可直接 ./ci.sh
	./ci.sh

dev-test:                 # 本地快速迭代:只跑测试,不跑 lint/漂移(完整判据仍是 ./ci.sh)
	uv run pytest

fmt:
	uv run ruff format .
	uv run ruff check --fix .

hooks:                    # 一次性:把同一条门钉在 push 前(不引入 pre-commit 框架)
	@test -d .git/hooks || { echo "不是 git 仓库(没有 .git/hooks/),先 git init。"; exit 1; }
	@printf '#!/bin/sh\nexec ./ci.sh\n' > .git/hooks/pre-push
	@chmod +x .git/hooks/pre-push
	@echo "✓ 已装 .git/hooks/pre-push → ./ci.sh"
```

`make hooks` 把同一条门装成 git `pre-push` 钩子。**不发 `.pre-commit-config.yaml`,
也不引入 pre-commit 框架**:那会让"该跑哪些检查"存在第二份清单,与 `ci.sh` 各自漂移,
而 pre-commit 的 mirrors 还会各自钉一套 ruff/mypy 版本,与 dev 组的版本对不上——
门最重要的属性是唯一且确定。一条 `exec ./ci.sh` 的 pre-push 钩子已经覆盖同样的场景。

### `ci.sh`(生成项目根,必须在仓库根跑)

```bash
#!/usr/bin/env bash
# 唯一的零警告门:人和 agent 共用的「是否完成」判据。任一项非零即整体失败。
# 必须从仓库根运行(深子目录下 pytest 会静默收集 0 个用例)。
set -euo pipefail

# 一律经 `uv run` 落到项目环境:`uv sync` 之后不必先 activate .venv,也不会误用全局工具。
uv run ruff format --check .
uv run ruff check .
uv run mypy .
uv run python scripts/check_conformance.py .   # 结构不变量(hook / 零 SDK 外泄 / 绝对导入…)
uv run python scripts/check_drift.py           # 文档/工件漂移守卫
uv run pytest                                  # 运行时类型(beartype)+ 行为 + 冒烟 + warnings-as-errors
echo "✓ 全绿"
```

一律走 `uv run`:`uv sync` 之后无需先 activate `.venv`,也不会静默用到全局装的 ruff/mypy/pytest
(版本一漂,门的结论就不可信)。

- 生产部署:设 `APP_BEARTYPE_ON=false`,beartype 完全不挂载,零开销。
- 边界安全(LLM/外部输入)始终由 §4 的 pydantic 兜底,与上述开关无关。
