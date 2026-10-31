/**
 * AF-5 交卷工具 · submit_verdict（doc-review 专用）
 *
 * 裁决走工具参数（结构化比对 verdict/route/issues），不再解析叙述后文本——
 * 裁决被截断 = 参数校验失败重试（引擎侧拒绝），DR-13「截断=本轮作废」随工具化自然达成。
 * 分发：devforge 渲染器按清单 extensions 声明拷入 profile 的 extensions/ 目录。
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const SubmitVerdictParams = Type.Object({
	verdict: Type.Union([Type.Literal("PASS"), Type.Literal("FAIL")], {
		description: "裁决：PASS = 单元通过；FAIL = 打回",
	}),
	route: Type.String({
		description:
			"打回阶段（phase0 清单 / phase1 勘察 / phase2 解释 / phase3 生成汇编）；PASS 时填 done",
	}),
	issues: Type.Optional(
		Type.Array(Type.String(), {
			description: "FAIL 时必填：问题清单，每条一个元素；PASS 时省略",
		}),
	),
});

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "submit_verdict",
		label: "Submit Verdict",
		description:
			"交裁决：审查完成后第一时间调用（先交裁决，再写报告正文落盘）。verdict/route/issues 走参数。",
		parameters: SubmitVerdictParams,
		async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
			const n = params.issues?.length ?? 0;
			return {
				content: [
					{
						type: "text",
						text: `裁决已交：${params.verdict}（route=${params.route}，issues=${n} 条）。接下来写审查报告正文并落盘。`,
					},
				],
			};
		},
	});
}
