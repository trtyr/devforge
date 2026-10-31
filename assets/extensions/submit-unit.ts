/**
 * AF-5 交卷工具 · submit_unit（doc-agent 专用）
 *
 * 调用本工具 = 交卷信号。引擎从 RPC 事件流捕获 toolName=submit_unit 的 args
 * （结构化比对 unit），不再依赖文本魔法块（<doc-complete> 容错全家桶随工具化退役）。
 * 分发：devforge 渲染器按清单 extensions 声明拷入 profile 的 extensions/ 目录，
 * pi 自动加载（jiti 直跑 TS）。
 */
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const SubmitUnitParams = Type.Object({
	unit: Type.String({
		description: "本单元编号（kickoff 给的，如 unit-3）。必须与 kickoff 一致，交错 = 判未完成",
	}),
});

export default function (pi: ExtensionAPI) {
	pi.registerTool({
		name: "submit_unit",
		label: "Submit Unit",
		description:
			"交卷：本单元写完、自检通过后调用。unit=本单元编号。调用即交卷信号，调用后停止输出等待审查（不要继续写任何文件）。",
		parameters: SubmitUnitParams,
		async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
			return {
				content: [
					{
						type: "text",
						text: `已交卷：${params.unit}。等待审查结果，不要继续写任何文件。`,
					},
				],
			};
		},
	});
}
