# 独立 dialog 应答器（测试侧设施）：监控隔离 HOME 的 plan 会话文件，
# 发现挂起的 ask_user_question toolCall → 提取问题/选项 → 探测 POST /ui/ui-N/answer 回填。
# 用法：python3 dialog_responder.py <隔离HOME路径> [port|auto]（默认 auto=扫 /proc 自动发现）
# 修复阶段后 SSE ui.request 投递仍待复测，本应答器与 SSE autopilot 互为冗余保障；
# 探测制回填：200=真实待答 dialog（fire-and-forget 修复后不再进入 pending，误答风险已消）。
import json, glob, time, sys, urllib.request

HOME = sys.argv[1] if len(sys.argv) > 1 else ""
PORT = sys.argv[2] if len(sys.argv) > 2 else "auto"
if not HOME:
    print("用法: python3 dialog_responder.py <隔离HOME路径> [port|auto]", file=sys.stderr)
    sys.exit(2)


def discover_port() -> str:
    """扫 /proc 找 HOME 匹配的 devforge 进程，读它的 DEVFORGE_ADDR。"""
    import glob as _g
    for envf in _g.glob("/proc/[0-9]*/environ"):
        try:
            env = open(envf, "rb").read().decode(errors="replace")
        except Exception:
            continue
        if f"HOME={HOME}" in env and "DEVFORGE_ADDR" in env:
            for tok in env.split("\x00"):
                if tok.startswith("DEVFORGE_ADDR="):
                    return tok.split("=", 1)[1].rsplit(":", 1)[-1]
    return ""


if PORT == "auto":
    PORT = discover_port()
    while not PORT:
        time.sleep(3)
        PORT = discover_port()
print(f"[responder] port={PORT}", flush=True)

BASE = f"http://127.0.0.1:{PORT}/api/v1"

answered_toolcalls = set()
next_probe = 1  # ui-N 探测游标（递增；已命中的不再重复探测）


def post_answer(ui_id: str, value: str) -> int | None:
    req = urllib.request.Request(
        f"{BASE}/ui/{ui_id}/answer",
        data=json.dumps({"value": value}).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return e.code
    except Exception:
        return None


def pick_value(questions: list) -> str:
    for q in questions:
        title = (q.get("header", "") or "") + (q.get("question", "") or "")
        opts = [o.get("label", "") for o in q.get("options", []) if isinstance(o, dict)]
        done_keys = ("完成", "是", "确认", "通过", "交付")
        if any(k in title for k in done_keys):
            hit = [l for l in opts if any(k in l for k in done_keys)]
            if hit:
                return hit[0]
        if opts:
            return opts[0]
    return "完成"


def watch():
    global next_probe
    last_size = 0
    while True:
        value = "完成"  # 默认盲答（探测制；无 SSE/会话信息时的保底）
        try:
            # 主信号：会话文件尾部的挂起 toolCall（能拿到问题/选项时智能应答）
            fs = sorted(glob.glob(f"{HOME}/.devforge/sessions/plan/*.jsonl"))
            if fs:
                lines = [l for l in open(fs[-1], errors="replace") if l.strip()]
                if lines:
                    d = json.loads(lines[-1])
                    m = d.get("message", {})
                    if isinstance(m, dict) and m.get("role") == "assistant":
                        for x in m.get("content", []):
                            if (
                                isinstance(x, dict)
                                and x.get("type") == "toolCall"
                                and x.get("name") == "ask_user_question"
                                and x.get("id") not in answered_toolcalls
                            ):
                                value = pick_value(x.get("arguments", {}).get("questions", []))
                                break
            # 探测制回填（主链路）：ui-N 游标轮询，200=真实待答——
            # 不依赖会话文件签名（dialog-pending 变体会话尾无 toolCall 行）
            # 也不依赖 SSE（P8-ISSUE-02：ui.request 不达 /events，SSE 消费者永久耳聋）
            for n in range(next_probe, next_probe + 6):
                code = post_answer(f"ui-{n}", value)
                if code == 200:
                    answered_toolcalls.add(f"probe-{n}")
                    next_probe = n + 1
                    print(f"[responder] ui-{n} <- {value[:40]} (probe)", flush=True)
                    break
                if code in (404, 500):
                    continue
        except Exception as e:
            print(f"[responder] err: {e}", flush=True)
        time.sleep(4)


print(f"[responder] watch {HOME} port {PORT}", flush=True)
watch()
