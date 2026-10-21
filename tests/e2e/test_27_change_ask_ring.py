#!/usr/bin/env python3
"""test_27 · 问询环（change-plan 真问用户）的 e2e 侧单测。

change-20261021-change-sop-path-line-src-config-manifest 的变更点里，有两块只在
e2e 侧可见、且**不需要真模型**就能证伪：

1. **装配语义**（`test_26_change_e2e.py` 的 `assemble_change_manifest`）：
   宿主清单已含 `change-*` 时，夹具条目必须**按名覆盖**宿主同名条目，
   且 change 六件套的 `extra_packages` 不被清空（ask 包要留着）。
   —— 旧写法 `extend + have` 在宿主已含 change-* 时是空操作 = 真链假绿。
2. **autopilot 的答复留痕**（`tests/e2e/common.py` 的 `UiAutoPilot`）：
   `answered` 记录里要带上**实际 POST 出去的 `value`**，且 select 形状的答复是
   **选项标签**（不是 `default_answer`）——真链的「答复 ↔ 计划」逐字核对靠它。

跑法：python3 tests/e2e/test_27_change_ask_ring.py   （失败即非零退出码）
"""

from __future__ import annotations

import json
import sys
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import UiAutoPilot  # noqa: E402
from test_26_change_e2e import assemble_change_manifest  # noqa: E402

HERE = Path(__file__).resolve().parent
FIXTURE = HERE / "fixtures" / "change_agents.json"

ASK_PKG = "npm:@juicesharp/rpiv-ask-user-question"


def load_fixture() -> list[dict]:
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


class AssembleChangeManifestTests(unittest.TestCase):
    """装配语义：夹具按名覆盖宿主同名条目。"""

    def test_host_manifest_already_containing_change_agents_is_overridden(self):
        """宿主清单已含 change-*（旧提示词 + 空包）→ 夹具按名覆盖它。

        错实现形态：退回 `extend + have`（宿主已含时是空操作）→ 本用例必须变红。
        """
        change_agents = load_fixture()
        host = {
            "agents": [
                {"name": "plan-agent", "role_prompt": "宿主版", "extra_packages": []},
                {
                    "name": "change-plan-agent",
                    "role_prompt": "宿主的旧提示词（没有问询契约）",
                    "extra_packages": [],
                },
                {
                    "name": "change-plan-review",
                    "role_prompt": "宿主的旧提示词",
                    "extra_packages": [],
                },
            ]
        }
        out = assemble_change_manifest(host, change_agents)
        names = [a["name"] for a in out["agents"]]
        self.assertEqual(len(names), len(set(names)), f"按名 upsert 不该产生重名：{names}")
        plan_agent = next(a for a in out["agents"] if a["name"] == "change-plan-agent")
        self.assertIn(
            "ask-user-question",
            plan_agent["role_prompt"],
            "宿主同名的旧提示词必须被夹具覆盖（否则测的不是内置提示词）",
        )
        self.assertIn(
            ASK_PKG,
            plan_agent["extra_packages"],
            f"change-plan-agent 的 ask 包不能被清空：{plan_agent['extra_packages']}",
        )
        for a in out["agents"]:
            if a["name"] in ("change-plan-agent", "change-coding-agent", "change-test-agent"):
                self.assertTrue(
                    a["extra_packages"],
                    f"{a['name']} 的 extra_packages 被清空了（ask 包会一起没）",
                )
            if a["name"] in ("change-plan-review", "change-coding-review", "change-test-review"):
                self.assertEqual(
                    a["extra_packages"],
                    [],
                    f"{a['name']} 是审查方（不与用户交互），不该有包",
                )

    def test_change_agents_are_appended_when_host_has_none(self):
        """宿主没有 change-* → 六个都被追加进去。

        错实现形态：忘了追加（只 upsert 不新增）→ 本用例必须变红。
        """
        change_agents = load_fixture()
        host = {"agents": [{"name": "plan-agent", "extra_packages": []}]}
        out = assemble_change_manifest(host, change_agents)
        names = [a["name"] for a in out["agents"]]
        for a in change_agents:
            self.assertIn(a["name"], names)
        self.assertEqual(len(names), 1 + len(change_agents))

    def test_other_host_agents_are_untouched(self):
        """宿主自己的 agent 不许被夹具动。

        错实现形态：装配时顺手改宿主条目 → 本用例必须变红。
        """
        change_agents = load_fixture()
        host = {
            "agents": [
                {"name": "plan-agent", "role_prompt": "宿主版", "extra_packages": ["x"]},
            ]
        }
        out = assemble_change_manifest(host, change_agents)
        plan_agent = next(a for a in out["agents"] if a["name"] == "plan-agent")
        self.assertEqual(plan_agent["role_prompt"], "宿主版")
        self.assertEqual(plan_agent["extra_packages"], ["x"])


class FakeServer:
    """只记录 POST 出去的答复（不连真服务器）。"""

    def __init__(self) -> None:
        self.posted: list[tuple[str, dict]] = []

    def req(self, method: str, path: str, body: dict | None = None, timeout: float = 10.0):
        self.posted.append((path, body or {}))
        return 200, {"ok": True}


class FakeSse:
    def __init__(self, events: list[dict]) -> None:
        self.events = events


def select_event(eid: str = "ui-1") -> dict:
    """一个 select 形状的 ui.request（question 内嵌裸 JSON，与真链同形）。"""
    payload = {
        "method": "select",
        "title": "复合词算一个词还是两个词？",
        "options": [{"label": "算一个词"}, {"label": "算两个词"}],
    }
    return {
        "type": "ui.request",
        "id": eid,
        "question": "问询环口径 " + json.dumps(payload, ensure_ascii=False),
    }


class UiAutoPilotAnswerTests(unittest.TestCase):
    """autopilot 的答复留痕：记下实际 POST 出去的 value。"""

    def test_smart_answer_picks_the_option_label_not_the_default_answer(self):
        """select 形状 → 答复是选项标签（不是 default_answer）。

        错实现形态：直接回 default_answer → 本用例必须变红。
        """
        pilot = UiAutoPilot(FakeServer(), FakeSse([]), default_answer="继续")
        answer = pilot._smart_answer(select_event())
        self.assertEqual(answer["value"], "算一个词")
        self.assertNotEqual(answer["value"], "继续")

    def test_answered_records_the_posted_value(self):
        """`answered` 记录里带实际 POST 出去的 value。

        错实现形态：记录里不记 value（真链无法做「答复 ↔ 计划」逐字核对）→ 本用例必须变红。
        """
        server = FakeServer()
        sse = FakeSse([select_event()])
        pilot = UiAutoPilot(server, sse, default_answer="继续")
        thread = threading.Thread(target=pilot._pump, daemon=True)
        thread.start()
        deadline = time.time() + 5.0
        while time.time() < deadline and not pilot.answered:
            time.sleep(0.05)
        pilot.stop()
        thread.join(timeout=2.0)

        self.assertTrue(pilot.answered, "autopilot 应当应答了 select 形状的请求")
        rec = pilot.answered[0]
        self.assertEqual(rec.get("value"), "算一个词", f"记录里的 value 应是选项标签：{rec}")
        self.assertEqual(rec["id"], "ui-1")
        self.assertEqual(rec["status"], 200)
        # 记下来的 value 必须就是真的 POST 出去的那一份
        self.assertEqual(len(server.posted), 1)
        path, body = server.posted[0]
        self.assertEqual(path, "/ui/ui-1/answer")
        self.assertEqual(body.get("value"), rec.get("value"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
